//! Schedule: which rule is due at a given moment and what is remembered about
//! it. The part that decides is pure; the caller supplies the clock, starts
//! the run and tells the user about skipped slots.
//!
//! A slot is one planned moment of a rule. Every slot is handled once: it runs,
//! is skipped with a journal entry, or passes silently.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use chrono::{Datelike, Duration, NaiveDateTime, NaiveTime};

use crate::error::{AppError, AppResult};
use crate::journal::Journal;
use crate::model::schedule::{
    DayWindow, RuleWhen, RunTarget, ScheduleRule, MAX_INTERVAL_MINUTES, MIN_INTERVAL_MINUTES,
};
use crate::storage::{valid_id, RuleState, ScheduleState, Store};

/// Slots and rule times: local, to the minute.
const SLOT_FORMAT: &str = "%Y-%m-%dT%H:%M";
/// When a rule appeared: to the second, so that a slot of the same minute that
/// had already passed does not fire.
const SINCE_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";
const TIME_FORMAT: &str = "%H:%M";
/// A slot seen this soon after its moment runs as planned.
const ON_TIME_SECONDS: i64 = 60;
const CATCH_UP_HOURS: i64 = 12;

fn parse_slot(text: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(text, SLOT_FORMAT).ok()
}

fn parse_since(text: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(text, SINCE_FORMAT).ok().or_else(|| parse_slot(text))
}

fn parse_time(text: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(text, TIME_FORMAT).ok()
}

/// Whether a rule can be stored: every time parses and the numbers are in range.
pub fn rule_valid(rule: &ScheduleRule) -> bool {
    let when_ok = match &rule.when {
        RuleWhen::Once { at } => parse_slot(at).is_some(),
        RuleWhen::Daily { time } => parse_time(time).is_some(),
        RuleWhen::Weekly { days, time } => {
            parse_time(time).is_some() && !days.is_empty() && days.iter().all(|day| (1..=7).contains(day))
        }
        RuleWhen::Interval { every_minutes, start_at, window } => {
            (MIN_INTERVAL_MINUTES..=MAX_INTERVAL_MINUTES).contains(every_minutes)
                && parse_slot(start_at).is_some()
                && window
                    .as_ref()
                    .is_none_or(|window| parse_time(&window.from).is_some() && parse_time(&window.to).is_some())
        }
    };
    when_ok && valid_id(&rule.id) && valid_id(&rule.target.id)
}

/// Inclusive on both ends; `from > to` wraps over midnight.
fn in_window(window: &DayWindow, time: NaiveTime) -> bool {
    match (parse_time(&window.from), parse_time(&window.to)) {
        (Some(from), Some(to)) if from <= to => from <= time && time <= to,
        (Some(from), Some(to)) => time >= from || time <= to,
        _ => false,
    }
}

/// The latest moment at or before `now` that the rule asks for.
fn latest_slot(when: &RuleWhen, now: NaiveDateTime) -> Option<NaiveDateTime> {
    match when {
        RuleWhen::Once { at } => parse_slot(at).filter(|at| *at <= now),
        RuleWhen::Daily { time } => {
            let today = now.date().and_time(parse_time(time)?);
            Some(if today <= now { today } else { today - Duration::days(1) })
        }
        RuleWhen::Weekly { days, time } => {
            let time = parse_time(time)?;
            (0..=7)
                .map(|back| (now.date() - Duration::days(back)).and_time(time))
                .find(|slot| *slot <= now && days.contains(&(slot.weekday().number_from_monday() as u8)))
        }
        RuleWhen::Interval { every_minutes, start_at, .. } => {
            let start = parse_slot(start_at)?;
            if now < start || *every_minutes < 1 {
                return None;
            }
            let passed = (now - start).num_minutes() / every_minutes;
            Some(start + Duration::minutes(passed * every_minutes))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Due {
    /// The slot has just come.
    OnTime(NaiveDateTime),
    /// A daily or weekly slot missed while the computer slept or ClickIt was closed.
    CatchUp(NaiveDateTime),
    /// The slot passed without a run and without a journal entry: it is too old
    /// or lies outside the time-of-day window.
    Passed(NaiveDateTime),
}

pub struct Moment {
    pub now: NaiveDateTime,
    /// The first look after a launch or after the computer slept.
    pub after_gap: bool,
    /// The "run a missed slot" setting.
    pub catch_up: bool,
}

/// What to do about the rule right now. `since` is when this version of the
/// rule appeared, `last_slot` is the slot handled last.
pub fn due(when: &RuleWhen, since: NaiveDateTime, last_slot: Option<NaiveDateTime>, moment: &Moment) -> Option<Due> {
    let slot = latest_slot(when, moment.now)?;
    if slot < since || last_slot.is_some_and(|last| slot <= last) {
        return None;
    }
    if let RuleWhen::Interval { window: Some(window), .. } = when {
        if !in_window(window, slot.time()) {
            return Some(Due::Passed(slot));
        }
    }
    let age = moment.now - slot;
    if age < Duration::seconds(ON_TIME_SECONDS) {
        return Some(Due::OnTime(slot));
    }
    let repeats_by_day = matches!(when, RuleWhen::Daily { .. } | RuleWhen::Weekly { .. });
    if repeats_by_day && moment.after_gap && moment.catch_up && age < Duration::hours(CATCH_UP_HOURS) {
        Some(Due::CatchUp(slot))
    } else {
        Some(Due::Passed(slot))
    }
}

/// What the scheduler needs from the rest of the app.
pub trait Host {
    /// Name of the scenario or queue; `None` when it was deleted.
    fn target_name(&self, target: &RunTarget) -> Option<String>;
    /// Starts the target the way a scheduled start goes: no countdown, no questions.
    fn start_scheduled(&self, target: &RunTarget) -> AppResult<()>;
    /// Shows that a slot was skipped.
    fn notify_skipped(&self, name: &str, reason: &AppError);
}

struct Inner {
    rules: Vec<ScheduleRule>,
    state: ScheduleState,
    /// Interval rules whose last slot was skipped: the next skip in a row stays quiet.
    skipped: HashSet<String>,
    last_tick: Option<NaiveDateTime>,
}

pub struct Scheduler {
    store: Arc<Store>,
    journal: Arc<Journal>,
    inner: Mutex<Inner>,
}

fn fingerprint(rule: &ScheduleRule) -> String {
    serde_json::to_string(&(rule.enabled, &rule.target, &rule.when)).unwrap_or_default()
}

/// Keeps the state of unchanged rules, starts anew for new and edited ones and
/// drops the state of deleted ones.
fn reconcile(rules: &[ScheduleRule], old: &ScheduleState, now: NaiveDateTime) -> ScheduleState {
    rules
        .iter()
        .map(|rule| {
            let fingerprint = fingerprint(rule);
            let state = match old.get(&rule.id) {
                Some(state) if state.fingerprint == fingerprint => state.clone(),
                _ => RuleState { fingerprint, since: now.format(SINCE_FORMAT).to_string(), last_slot: None },
            };
            (rule.id.clone(), state)
        })
        .collect()
}

impl Scheduler {
    pub fn new(store: Arc<Store>, journal: Arc<Journal>, now: NaiveDateTime) -> Self {
        let rules = store.load_schedule();
        let state = reconcile(&rules, &store.load_schedule_state(), now);
        let _ = store.save_schedule_state(&state);
        Self { store, journal, inner: Mutex::new(Inner { rules, state, skipped: HashSet::new(), last_tick: None }) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn rules(&self) -> Vec<ScheduleRule> {
        self.lock().rules.clone()
    }

    /// Replaces the rules. A list with a broken rule is rejected as a whole.
    pub fn set_rules(&self, rules: Vec<ScheduleRule>, now: NaiveDateTime) -> AppResult<()> {
        let mut ids = HashSet::new();
        if !rules.iter().all(|rule| rule_valid(rule) && ids.insert(rule.id.as_str())) {
            return Err(AppError::new("scheduleInvalid"));
        }
        self.store.save_schedule(&rules)?;
        let mut inner = self.lock();
        inner.state = reconcile(&rules, &inner.state, now);
        let known: HashSet<&str> = rules.iter().map(|rule| rule.id.as_str()).collect();
        inner.skipped.retain(|id| known.contains(id.as_str()));
        let saved = self.store.save_schedule_state(&inner.state);
        inner.rules = rules;
        saved
    }

    /// Drops the rules of a deleted scenario or queue. Returns whether anything changed.
    pub fn forget_target(&self, target: &RunTarget, now: NaiveDateTime) -> bool {
        let rules = self.rules();
        let kept: Vec<ScheduleRule> = rules.iter().filter(|rule| rule.target != *target).cloned().collect();
        kept.len() != rules.len() && self.set_rules(kept, now).is_ok()
    }

    /// Looks at every rule once. Called about once a second.
    pub fn tick(&self, now: NaiveDateTime, catch_up: bool, host: &dyn Host) {
        // What is due is collected under the lock and acted upon after it is released.
        let due_now: Vec<(ScheduleRule, Due)> = {
            let mut inner = self.lock();
            let after_gap = inner
                .last_tick
                .is_none_or(|last| now < last || now - last > Duration::seconds(ON_TIME_SECONDS));
            inner.last_tick = Some(now);
            let moment = Moment { now, after_gap, catch_up };
            let found: Vec<(ScheduleRule, Due)> = inner
                .rules
                .iter()
                .filter(|rule| rule.enabled)
                .filter_map(|rule| {
                    let state = inner.state.get(&rule.id)?;
                    let since = parse_since(&state.since)?;
                    let last = state.last_slot.as_deref().and_then(parse_slot);
                    due(&rule.when, since, last, &moment).map(|due| (rule.clone(), due))
                })
                .collect();
            for (rule, due) in &found {
                let (Due::OnTime(slot) | Due::CatchUp(slot) | Due::Passed(slot)) = due;
                if let Some(state) = inner.state.get_mut(&rule.id) {
                    state.last_slot = Some(slot.format(SLOT_FORMAT).to_string());
                }
            }
            if !found.is_empty() {
                let _ = self.store.save_schedule_state(&inner.state);
            }
            found
        };

        for (rule, due) in due_now {
            let caught_up = match due {
                Due::OnTime(_) => false,
                Due::CatchUp(_) => true,
                Due::Passed(_) => continue,
            };
            self.fire(&rule, caught_up, host);
        }
    }

    fn fire(&self, rule: &ScheduleRule, caught_up: bool, host: &dyn Host) {
        let Some(name) = host.target_name(&rule.target) else {
            self.journal.add("scheduleMissing", &[]);
            return;
        };
        match host.start_scheduled(&rule.target) {
            Ok(()) => {
                self.lock().skipped.remove(&rule.id);
                let code = if caught_up { "scheduleCatchUp" } else { "scheduleFired" };
                self.journal.add(code, &[("name", name)]);
            }
            Err(reason) => {
                self.journal.add_with_reason("scheduleSkipped", &[("name", name.clone())], &reason);
                // An interval rule that keeps hitting a long run reports only the first skip in a row.
                let repeated = matches!(rule.when, RuleWhen::Interval { .. })
                    && !self.lock().skipped.insert(rule.id.clone());
                if !repeated {
                    host.notify_skipped(&name, &reason);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, SINCE_FORMAT).unwrap()
    }

    fn moment(now: &str) -> Moment {
        Moment { now: at(now), after_gap: false, catch_up: true }
    }

    fn after_gap(now: &str) -> Moment {
        Moment { now: at(now), after_gap: true, catch_up: true }
    }

    const SINCE: &str = "2026-11-01T00:00:00";

    #[test]
    fn daily_rule_fires_once_in_its_minute() {
        let when = RuleWhen::Daily { time: "09:30".to_string() };
        let since = at(SINCE);
        assert_eq!(due(&when, since, None, &moment("2026-11-17T09:29:59")), Some(Due::Passed(at("2026-11-16T09:30:00"))));
        let slot = at("2026-11-17T09:30:00");
        assert_eq!(due(&when, since, Some(at("2026-11-16T09:30:00")), &moment("2026-11-17T09:30:01")), Some(Due::OnTime(slot)));
        // The same slot is never handled twice.
        assert_eq!(due(&when, since, Some(slot), &moment("2026-11-17T09:30:30")), None);
        assert_eq!(due(&when, since, Some(slot), &moment("2026-11-17T23:00:00")), None);
    }

    #[test]
    fn missed_daily_slot_is_caught_up_within_twelve_hours_after_a_gap() {
        let when = RuleWhen::Daily { time: "09:30".to_string() };
        let since = at(SINCE);
        let slot = at("2026-11-17T09:30:00");
        assert_eq!(due(&when, since, None, &after_gap("2026-11-17T15:00:00")), Some(Due::CatchUp(slot)));
        assert_eq!(due(&when, since, None, &after_gap("2026-11-17T21:30:00")), Some(Due::Passed(slot)));
        // Without a gap a late look means the slot was simply not seen in time.
        assert_eq!(due(&when, since, None, &moment("2026-11-17T15:00:00")), Some(Due::Passed(slot)));
        let off = Moment { now: at("2026-11-17T15:00:00"), after_gap: true, catch_up: false };
        assert_eq!(due(&when, since, None, &off), Some(Due::Passed(slot)));
    }

    #[test]
    fn slots_older_than_the_rule_do_not_fire() {
        let when = RuleWhen::Daily { time: "09:30".to_string() };
        // The rule was saved twenty seconds into its own minute.
        assert_eq!(due(&when, at("2026-11-17T09:30:20"), None, &after_gap("2026-11-17T09:30:21")), None);
        assert_eq!(due(&when, at("2026-11-17T09:31:00"), None, &after_gap("2026-11-17T15:00:00")), None);
    }

    #[test]
    fn weekly_rule_takes_only_its_days() {
        // 2026-11-17 is a Tuesday.
        let when = RuleWhen::Weekly { days: vec![1, 5], time: "08:00".to_string() };
        let since = at(SINCE);
        assert_eq!(
            due(&when, since, None, &moment("2026-11-17T08:00:10")),
            Some(Due::Passed(at("2026-11-16T08:00:00")))
        );
        assert_eq!(
            due(&when, since, Some(at("2026-11-16T08:00:00")), &moment("2026-11-20T08:00:10")),
            Some(Due::OnTime(at("2026-11-20T08:00:00")))
        );
        assert_eq!(
            due(&when, since, None, &after_gap("2026-11-16T12:00:00")),
            Some(Due::CatchUp(at("2026-11-16T08:00:00")))
        );
    }

    #[test]
    fn one_time_rule_is_never_caught_up() {
        let when = RuleWhen::Once { at: "2026-11-17T10:00".to_string() };
        let since = at(SINCE);
        assert_eq!(due(&when, since, None, &moment("2026-11-17T09:59:59")), None);
        assert_eq!(due(&when, since, None, &moment("2026-11-17T10:00:00")), Some(Due::OnTime(at("2026-11-17T10:00:00"))));
        assert_eq!(due(&when, since, None, &after_gap("2026-11-17T10:05:00")), Some(Due::Passed(at("2026-11-17T10:00:00"))));
    }

    #[test]
    fn interval_rule_follows_the_grid_from_its_start() {
        let when = RuleWhen::Interval { every_minutes: 15, start_at: "2026-11-17T10:07".to_string(), window: None };
        let since = at(SINCE);
        assert_eq!(due(&when, since, None, &moment("2026-11-17T10:06:59")), None);
        assert_eq!(due(&when, since, None, &moment("2026-11-17T10:07:00")), Some(Due::OnTime(at("2026-11-17T10:07:00"))));
        let last = Some(at("2026-11-17T10:07:00"));
        assert_eq!(due(&when, since, last, &moment("2026-11-17T10:21:59")), None);
        assert_eq!(due(&when, since, last, &moment("2026-11-17T10:22:00")), Some(Due::OnTime(at("2026-11-17T10:22:00"))));
        // After a sleep the missed slots are not run.
        assert_eq!(due(&when, since, last, &after_gap("2026-11-17T12:30:00")), Some(Due::Passed(at("2026-11-17T12:22:00"))));
    }

    #[test]
    fn interval_slots_outside_the_day_window_pass_silently() {
        let window = |from: &str, to: &str| Some(DayWindow { from: from.to_string(), to: to.to_string() });
        let rule = |window| RuleWhen::Interval { every_minutes: 60, start_at: "2026-11-17T00:00".to_string(), window };
        let since = at(SINCE);
        let day = rule(window("09:00", "18:00"));
        assert_eq!(due(&day, since, None, &moment("2026-11-17T08:00:05")), Some(Due::Passed(at("2026-11-17T08:00:00"))));
        assert_eq!(due(&day, since, None, &moment("2026-11-17T09:00:05")), Some(Due::OnTime(at("2026-11-17T09:00:00"))));
        assert_eq!(due(&day, since, None, &moment("2026-11-17T18:00:05")), Some(Due::OnTime(at("2026-11-17T18:00:00"))));
        let night = rule(window("22:00", "06:00"));
        assert_eq!(due(&night, since, None, &moment("2026-11-17T23:00:05")), Some(Due::OnTime(at("2026-11-17T23:00:00"))));
        assert_eq!(due(&night, since, None, &moment("2026-11-17T03:00:05")), Some(Due::OnTime(at("2026-11-17T03:00:00"))));
        assert_eq!(due(&night, since, None, &moment("2026-11-17T12:00:05")), Some(Due::Passed(at("2026-11-17T12:00:00"))));
    }

    #[test]
    fn broken_rules_are_rejected() {
        let target = RunTarget { kind: crate::model::schedule::TargetKind::Scenario, id: "sc1".to_string() };
        let rule = |when| ScheduleRule { id: "r1".to_string(), enabled: true, target: target.clone(), when };
        assert!(rule_valid(&rule(RuleWhen::Daily { time: "07:05".to_string() })));
        assert!(!rule_valid(&rule(RuleWhen::Daily { time: "25:00".to_string() })));
        assert!(!rule_valid(&rule(RuleWhen::Weekly { days: vec![], time: "07:05".to_string() })));
        assert!(!rule_valid(&rule(RuleWhen::Weekly { days: vec![8], time: "07:05".to_string() })));
        assert!(!rule_valid(&rule(RuleWhen::Once { at: "tomorrow".to_string() })));
        assert!(!rule_valid(&rule(RuleWhen::Interval {
            every_minutes: 0,
            start_at: "2026-11-17T10:00".to_string(),
            window: None
        })));
    }

    #[test]
    fn edited_rule_starts_anew_and_unchanged_one_keeps_its_state() {
        let target = RunTarget { kind: crate::model::schedule::TargetKind::Scenario, id: "sc1".to_string() };
        let rule = |id: &str, time: &str| ScheduleRule {
            id: id.to_string(),
            enabled: true,
            target: target.clone(),
            when: RuleWhen::Daily { time: time.to_string() },
        };
        let first = reconcile(&[rule("a", "09:00"), rule("b", "10:00")], &ScheduleState::new(), at(SINCE));
        let mut fired = first.clone();
        fired.get_mut("a").unwrap().last_slot = Some("2026-11-02T09:00".to_string());
        fired.get_mut("b").unwrap().last_slot = Some("2026-11-02T10:00".to_string());

        let next = reconcile(&[rule("a", "09:00"), rule("b", "11:00")], &fired, at("2026-11-03T12:00:00"));
        assert_eq!(next["a"], fired["a"]);
        assert_eq!(next["b"].last_slot, None);
        assert_eq!(next["b"].since, "2026-11-03T12:00:00");
        assert_eq!(reconcile(&[rule("a", "09:00")], &next, at(SINCE)).len(), 1);
    }
}
