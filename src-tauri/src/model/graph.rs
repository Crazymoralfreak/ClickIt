//! The canvas form of a scenario: blocks, the links between their outputs and
//! the notes drawn around them. The engine and the checks read blocks and
//! links only; positions, bend points and notes belong to the editor.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::scenario::{
    new_id, ClickAction, Condition, KeyAction, LoopMode, Modifiers, MouseButton, OnFound, OnTimeout, Pause, Position,
    Repeat, ScrollDirection, Step, StepKind,
};

/// Written into a scenario that holds a graph. Lists keep version 1.
pub const GRAPH_VERSION: u32 = 2;
pub const MAX_INTERCEPTS: usize = 8;
/// How many "Run scenario" blocks may be nested inside one another.
pub const MAX_CALL_DEPTH: usize = 8;
pub const DEFAULT_INTERCEPT_MS: i64 = 2000;
pub const MIN_INTERCEPT_MS: i64 = 500;
/// Poll interval given to conditions that a list checked without one.
const DEFAULT_POLL_MS: i64 = 200;

/// Distance between blocks placed by the automatic layout.
const COLUMN: f64 = 280.0;
const ROW: f64 = 140.0;
/// Size of a block on the canvas, for a frame drawn around blocks.
pub const BLOCK_WIDTH: f64 = 220.0;
pub const BLOCK_HEIGHT: f64 = 84.0;
const FRAME_MARGIN: f64 = 24.0;
/// Room for the title of a frame above its blocks.
const FRAME_TITLE: f64 = 36.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Port {
    Next,
    Timeout,
    Yes,
    No,
    Body,
    Done,
}

impl Port {
    pub fn as_str(self) -> &'static str {
        match self {
            Port::Next => "next",
            Port::Timeout => "timeout",
            Port::Yes => "yes",
            Port::No => "no",
            Port::Body => "body",
            Port::Done => "done",
        }
    }
}

/// Action blocks carry the same fields as the steps of a list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum NodeKind {
    /// Where a pass begins. Exactly one per graph.
    Start,
    Click {
        button: MouseButton,
        action: ClickAction,
        position: Position,
    },
    Move {
        position: Position,
    },
    Drag {
        button: MouseButton,
        from: Position,
        to: Position,
    },
    Scroll {
        direction: ScrollDirection,
        notches: i64,
        position: Position,
    },
    Key {
        key: String,
        action: KeyAction,
        #[serde(default)]
        modifiers: Modifiers,
    },
    Text {
        text: String,
    },
    Pause {
        duration: Pause,
    },
    /// Ends the run like the Stop button.
    Stop,
    /// `timeout_ms == 0` waits until the run is stopped by hand. An unwired
    /// `timeout` output fails the scenario.
    Wait {
        condition: Condition,
        poll_ms: i64,
        timeout_ms: i64,
        #[serde(default)]
        on_found: OnFound,
    },
    /// `timeout_ms == 0` checks the condition once, right away.
    Branch {
        condition: Condition,
        poll_ms: i64,
        timeout_ms: i64,
    },
    /// Lets the pass into `body` the given number of times, then into `done`.
    Counter {
        times: i64,
        #[serde(default)]
        pause_between: Pause,
    },
    /// Runs one pass of another scenario.
    Call {
        #[serde(default)]
        scenario_id: String,
    },
    /// Watches for a condition on top of the main flow. Has no input.
    Intercept {
        condition: Condition,
        #[serde(default = "default_intercept_ms")]
        min_interval_ms: i64,
    },
}

fn default_intercept_ms() -> i64 {
    DEFAULT_INTERCEPT_MS
}

impl NodeKind {
    /// Outputs of the block, in the order the layout follows them.
    pub fn ports(&self) -> &'static [Port] {
        match self {
            NodeKind::Wait { .. } => &[Port::Next, Port::Timeout],
            NodeKind::Branch { .. } => &[Port::Yes, Port::No],
            NodeKind::Counter { .. } => &[Port::Body, Port::Done],
            NodeKind::Stop => &[],
            NodeKind::Start
            | NodeKind::Click { .. }
            | NodeKind::Move { .. }
            | NodeKind::Drag { .. }
            | NodeKind::Scroll { .. }
            | NodeKind::Key { .. }
            | NodeKind::Text { .. }
            | NodeKind::Pause { .. }
            | NodeKind::Call { .. }
            | NodeKind::Intercept { .. } => &[Port::Next],
        }
    }

    /// The output a switched-off block is passed through.
    pub fn skip_port(&self) -> Port {
        match self {
            NodeKind::Branch { .. } => Port::No,
            NodeKind::Counter { .. } => Port::Done,
            _ => Port::Next,
        }
    }

    /// Start and intercept blocks begin a flow; nothing may lead into them.
    pub fn is_entry(&self) -> bool {
        matches!(self, NodeKind::Start | NodeKind::Intercept { .. })
    }

    /// The same action as a step of a list, for the blocks that have one.
    pub fn action(&self) -> Option<StepKind> {
        Some(match self.clone() {
            NodeKind::Click { button, action, position } => StepKind::Click { button, action, position },
            NodeKind::Move { position } => StepKind::Move { position },
            NodeKind::Drag { button, from, to } => StepKind::Drag { button, from, to },
            NodeKind::Scroll { direction, notches, position } => StepKind::Scroll { direction, notches, position },
            NodeKind::Key { key, action, modifiers } => StepKind::Key { key, action, modifiers },
            NodeKind::Text { text } => StepKind::Text { text },
            NodeKind::Pause { duration } => StepKind::Pause { duration },
            NodeKind::Start
            | NodeKind::Stop
            | NodeKind::Wait { .. }
            | NodeKind::Branch { .. }
            | NodeKind::Counter { .. }
            | NodeKind::Call { .. }
            | NodeKind::Intercept { .. } => return None,
        })
    }

    pub fn condition(&self) -> Option<&Condition> {
        match self {
            NodeKind::Wait { condition, .. }
            | NodeKind::Branch { condition, .. }
            | NodeKind::Intercept { condition, .. } => Some(condition),
            _ => None,
        }
    }

    /// The block for a simple step of a list; `None` for the blocks a graph
    /// expresses with links.
    fn from_step(kind: &StepKind) -> Option<NodeKind> {
        Some(match kind.clone() {
            StepKind::Click { button, action, position } => NodeKind::Click { button, action, position },
            StepKind::Move { position } => NodeKind::Move { position },
            StepKind::Drag { button, from, to } => NodeKind::Drag { button, from, to },
            StepKind::Scroll { direction, notches, position } => NodeKind::Scroll { direction, notches, position },
            StepKind::Key { key, action, modifiers } => NodeKind::Key { key, action, modifiers },
            StepKind::Text { text } => NodeKind::Text { text },
            StepKind::Pause { duration } => NodeKind::Pause { duration },
            StepKind::Stop => NodeKind::Stop,
            StepKind::Wait { condition, poll_ms, timeout_ms, on_found, .. } => {
                NodeKind::Wait { condition, poll_ms, timeout_ms, on_found }
            }
            StepKind::If { .. } | StepKind::Loop { .. } | StepKind::BreakLoop => return None,
        })
    }
}

fn enabled_default() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    /// Empty until a save fills it in.
    #[serde(default)]
    pub id: String,
    /// The user's own name of the block, shown instead of its type.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// A block saved without a position is placed by the automatic layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
    #[serde(default)]
    pub pause_after: Pause,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<Repeat>,
    #[serde(flatten)]
    pub kind: NodeKind,
}

impl Node {
    pub fn new(kind: NodeKind) -> Self {
        Self {
            id: new_id("n"),
            title: String::new(),
            x: None,
            y: None,
            enabled: true,
            pause_after: Pause::default(),
            repeat: None,
            kind,
        }
    }

    fn placed(&self) -> bool {
        self.x.is_some() && self.y.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Edge {
    pub from: String,
    pub port: Port,
    pub to: String,
    /// Bend points the user put on the link.
    #[serde(default)]
    pub points: Vec<Point>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NoteKind {
    #[default]
    Note,
    Frame,
}

/// A note or a frame. Neither the engine nor the checks read them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub kind: NoteKind,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub width: f64,
    #[serde(default)]
    pub height: f64,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub color: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Graph {
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub notes: Vec<Note>,
}

/// Where a pass goes after a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Follow {
    /// Index of the next block.
    To(usize),
    /// The output is not linked: the pass ends here.
    End,
    /// The `timeout` output is not linked: the scenario fails.
    Timeout,
}

/// Which output a counter block takes after `passed` passes into its body.
pub fn counter_port(times: i64, passed: i64) -> Port {
    if passed < times {
        Port::Body
    } else {
        Port::Done
    }
}

/// Links of a graph in the form a walk needs.
pub struct Wiring {
    index: HashMap<String, usize>,
    out: HashMap<(usize, Port), usize>,
}

impl Wiring {
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.index.get(id).copied()
    }

    pub fn target(&self, from: usize, port: Port) -> Option<usize> {
        self.out.get(&(from, port)).copied()
    }

    pub fn follow(&self, from: usize, port: Port) -> Follow {
        match self.target(from, port) {
            Some(to) => Follow::To(to),
            None if port == Port::Timeout => Follow::Timeout,
            None => Follow::End,
        }
    }
}

impl Graph {
    /// A graph with nothing but its start block.
    pub fn new() -> Self {
        let mut start = Node::new(NodeKind::Start);
        start.x = Some(0.0);
        start.y = Some(0.0);
        Self { nodes: vec![start], edges: Vec::new(), notes: Vec::new() }
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|node| node.id == id)
    }

    /// Index of the start block, when there is exactly one to begin from.
    pub fn start(&self) -> Option<usize> {
        self.nodes.iter().position(|node| matches!(node.kind, NodeKind::Start))
    }

    /// Blocks that count towards the size limit and the list of scenarios.
    pub fn block_count(&self) -> usize {
        self.nodes.iter().filter(|node| !matches!(node.kind, NodeKind::Start)).count()
    }

    /// The first link of every output wins; the checks report the rest.
    pub fn wiring(&self) -> Wiring {
        let mut index = HashMap::with_capacity(self.nodes.len());
        for (at, node) in self.nodes.iter().enumerate() {
            index.entry(node.id.clone()).or_insert(at);
        }
        let mut out = HashMap::with_capacity(self.edges.len());
        for edge in &self.edges {
            if let (Some(from), Some(to)) = (index.get(&edge.from), index.get(&edge.to)) {
                out.entry((*from, edge.port)).or_insert(*to);
            }
        }
        Wiring { index, out }
    }

    /// Number and name of a block for the journal and for messages: `3 «Wait for Start»`.
    pub fn label(&self, index: usize) -> String {
        let number = index + 1;
        match self.nodes.get(index).map(|node| node.title.trim()) {
            Some(title) if !title.is_empty() => format!("{number} «{title}»"),
            _ => number.to_string(),
        }
    }

    /// Indexes of the blocks a pass can get to from `from`, itself included.
    pub fn reachable(&self, wiring: &Wiring, from: usize) -> HashSet<usize> {
        let mut seen = HashSet::new();
        let mut stack = vec![from];
        while let Some(at) = stack.pop() {
            if !seen.insert(at) {
                continue;
            }
            let Some(node) = self.nodes.get(at) else {
                continue;
            };
            for port in node.kind.ports() {
                if let Some(to) = wiring.target(at, *port) {
                    stack.push(to);
                }
            }
        }
        seen
    }

    /// Ids of the scenarios the graph runs, without repeats.
    pub fn callees(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for node in &self.nodes {
            if let NodeKind::Call { scenario_id } = &node.kind {
                if !scenario_id.is_empty() && !out.contains(scenario_id) {
                    out.push(scenario_id.clone());
                }
            }
        }
        out
    }

    pub fn collect_templates(&self, out: &mut Vec<String>) {
        for node in &self.nodes {
            let name = match node.kind.condition() {
                Some(Condition::ImageFound { template, .. }) | Some(Condition::ImageGone { template, .. }) => template,
                _ => continue,
            };
            if !name.is_empty() {
                out.push(name.clone());
            }
        }
    }

    pub fn fill_empty_ids(&mut self) {
        for node in &mut self.nodes {
            if node.id.is_empty() {
                node.id = new_id("n");
            }
        }
        for note in &mut self.notes {
            if note.id.is_empty() {
                note.id = new_id("t");
            }
        }
    }

    /// Gives every block and note a fresh id and moves the links over.
    pub fn renew_ids(&mut self) {
        let mut renamed: HashMap<String, String> = HashMap::with_capacity(self.nodes.len());
        for node in &mut self.nodes {
            let fresh = new_id("n");
            renamed.entry(std::mem::replace(&mut node.id, fresh.clone())).or_insert(fresh);
        }
        for edge in &mut self.edges {
            if let Some(id) = renamed.get(&edge.from) {
                edge.from = id.clone();
            }
            if let Some(id) = renamed.get(&edge.to) {
                edge.to = id.clone();
            }
        }
        for note in &mut self.notes {
            note.id = new_id("t");
        }
    }

    /// Points every "Run scenario" block at the new id of what it ran.
    pub fn remap_calls(&mut self, renamed: &HashMap<String, String>) {
        for node in &mut self.nodes {
            if let NodeKind::Call { scenario_id } = &mut node.kind {
                if let Some(id) = renamed.get(scenario_id) {
                    *scenario_id = id.clone();
                }
            }
        }
    }

    /// Places every block: the first output goes straight down, the others
    /// open a new column to the right.
    pub fn arrange(&mut self) {
        let spots = self.spots();
        for (node, (column, row)) in self.nodes.iter_mut().zip(spots) {
            node.x = Some(column as f64 * COLUMN);
            node.y = Some(row as f64 * ROW);
        }
    }

    /// Places only the blocks that came without a position, below the ones
    /// that have one. A graph without any positions is arranged as a whole.
    pub fn place_missing(&mut self) {
        if self.nodes.iter().all(Node::placed) {
            return;
        }
        if !self.nodes.iter().any(Node::placed) {
            self.arrange();
            return;
        }
        let spots = self.spots();
        let bottom = self.nodes.iter().filter(|node| node.placed()).filter_map(|node| node.y).fold(f64::MIN, f64::max);
        let left = self.nodes.iter().filter(|node| node.placed()).filter_map(|node| node.x).fold(f64::MAX, f64::min);
        let missing = |node: &Node| !node.placed();
        let first_row = self.nodes.iter().zip(&spots).filter(|(node, _)| missing(node)).map(|(_, spot)| spot.1).min();
        let first_column =
            self.nodes.iter().zip(&spots).filter(|(node, _)| missing(node)).map(|(_, spot)| spot.0).min();
        let (Some(first_row), Some(first_column)) = (first_row, first_column) else {
            return;
        };
        for (node, (column, row)) in self.nodes.iter_mut().zip(spots) {
            if !node.placed() {
                node.x = Some(left + (column - first_column) as f64 * COLUMN);
                node.y = Some(bottom + ROW + (row - first_row) as f64 * ROW);
            }
        }
    }

    /// Column and row of every block. Flows begin at the start block, at the
    /// intercepts and then at whatever is left, in the order of the file.
    fn spots(&self) -> Vec<(usize, usize)> {
        let wiring = self.wiring();
        let mut spots: Vec<Option<(usize, usize)>> = vec![None; self.nodes.len()];
        let mut next_column = 0usize;
        let roots = (0..self.nodes.len())
            .filter(|at| matches!(self.nodes[*at].kind, NodeKind::Start))
            .chain((0..self.nodes.len()).filter(|at| matches!(self.nodes[*at].kind, NodeKind::Intercept { .. })))
            .chain(0..self.nodes.len());
        for root in roots {
            if spots[root].is_some() {
                continue;
            }
            // Depth first, so a branch is laid out before the next one opens its column.
            let mut stack = vec![(root, next_column, 0usize)];
            next_column += 1;
            while let Some((at, column, row)) = stack.pop() {
                if spots[at].is_some() {
                    continue;
                }
                spots[at] = Some((column, row));
                let targets: Vec<usize> =
                    self.nodes[at].kind.ports().iter().filter_map(|port| wiring.target(at, *port)).collect();
                let mut pending = Vec::new();
                for (order, to) in targets.into_iter().enumerate() {
                    if spots[to].is_some() {
                        continue;
                    }
                    if order == 0 {
                        pending.push((to, column, row + 1));
                    } else {
                        pending.push((to, next_column, row + 1));
                        next_column += 1;
                    }
                }
                // The straight-down output is popped first.
                stack.extend(pending.into_iter().rev());
            }
        }
        spots.into_iter().map(|spot| spot.unwrap_or((0, 0))).collect()
    }

    /// Rectangle of a frame that holds the given blocks: x, y, width, height.
    /// `None` when none of them is placed.
    pub fn frame_around(&self, ids: &[String]) -> Option<(f64, f64, f64, f64)> {
        let spots: Vec<(f64, f64)> = self
            .nodes
            .iter()
            .filter(|node| ids.contains(&node.id))
            .filter_map(|node| Some((node.x?, node.y?)))
            .collect();
        let (first, rest) = spots.split_first()?;
        let (mut left, mut top, mut right, mut bottom) = (first.0, first.1, first.0, first.1);
        for (x, y) in rest {
            left = left.min(*x);
            top = top.min(*y);
            right = right.max(*x);
            bottom = bottom.max(*y);
        }
        Some((
            left - FRAME_MARGIN,
            top - FRAME_MARGIN - FRAME_TITLE,
            right - left + BLOCK_WIDTH + FRAME_MARGIN * 2.0,
            bottom - top + BLOCK_HEIGHT + FRAME_MARGIN * 2.0 + FRAME_TITLE,
        ))
    }

    /// Moves the chosen blocks into a graph of their own and leaves a "Run
    /// scenario" block in their place. The pass has to enter the chosen blocks
    /// at one block and to leave them towards one block (or to end inside
    /// them), otherwise one output of the new block could not stand for them.
    /// The error is the code of what is in the way.
    pub fn extract(&mut self, ids: &[String], callee_id: &str, title: &str) -> Result<Graph, &'static str> {
        let chosen: HashSet<String> =
            self.nodes.iter().filter(|node| ids.contains(&node.id)).map(|node| node.id.clone()).collect();
        if chosen.is_empty() {
            return Err("extractEmpty");
        }
        if self.nodes.iter().any(|node| chosen.contains(&node.id) && node.kind.is_entry()) {
            return Err("extractEntry");
        }
        let inside = |id: &str| chosen.contains(id);

        let mut entries: Vec<String> = Vec::new();
        for edge in self.edges.iter().filter(|edge| !inside(&edge.from) && inside(&edge.to)) {
            if !entries.contains(&edge.to) {
                entries.push(edge.to.clone());
            }
        }
        if entries.is_empty() {
            // Nothing leads here from outside: the pass begins where nothing leads from inside either.
            entries = self
                .nodes
                .iter()
                .filter(|node| inside(&node.id))
                .filter(|node| !self.edges.iter().any(|edge| edge.to == node.id && edge.from != node.id))
                .map(|node| node.id.clone())
                .collect();
        }
        let [entry] = entries.as_slice() else {
            return Err("extractManyEntries");
        };
        let entry = entry.clone();

        // `None` stands for "the pass ends here".
        let mut exits: Vec<Option<String>> = Vec::new();
        for edge in self.edges.iter().filter(|edge| inside(&edge.from) && !inside(&edge.to)) {
            if !exits.contains(&Some(edge.to.clone())) {
                exits.push(Some(edge.to.clone()));
            }
        }
        let ends_inside = self.nodes.iter().filter(|node| inside(&node.id)).any(|node| {
            node.kind.ports().iter().filter(|port| **port != Port::Timeout).any(|port| {
                !self.edges.iter().any(|edge| edge.from == node.id && edge.port == *port)
            })
        });
        if ends_inside {
            exits.push(None);
        }
        if exits.len() > 1 {
            return Err("extractManyExits");
        }
        let exit = exits.into_iter().next().flatten();

        let (moved, mut kept): (Vec<Node>, Vec<Node>) =
            std::mem::take(&mut self.nodes).into_iter().partition(|node| inside(&node.id));
        let place = moved.iter().find(|node| node.id == entry).map(|node| (node.x, node.y)).unwrap_or((None, None));
        let mut call = Node::new(NodeKind::Call { scenario_id: callee_id.to_string() });
        call.title = title.to_string();
        (call.x, call.y) = place;

        let mut inner = Vec::new();
        let mut outer = Vec::new();
        for mut edge in std::mem::take(&mut self.edges) {
            match (inside(&edge.from), inside(&edge.to)) {
                (true, true) => inner.push(edge),
                (false, true) => {
                    edge.to = call.id.clone();
                    edge.points.clear();
                    outer.push(edge);
                }
                (true, false) => {}
                (false, false) => outer.push(edge),
            }
        }
        if let Some(to) = exit {
            outer.push(Edge { from: call.id.clone(), port: Port::Next, to, points: Vec::new() });
        }
        kept.push(call);
        self.nodes = kept;
        self.edges = outer;

        let mut start = Node::new(NodeKind::Start);
        start.x = place.0;
        start.y = place.1.map(|y| y - ROW);
        inner.insert(0, Edge { from: start.id.clone(), port: Port::Next, to: entry, points: Vec::new() });
        let mut nodes = vec![start];
        nodes.extend(moved);
        Ok(Graph { nodes, edges: inner, notes: Vec::new() })
    }

    /// Adds recorded steps as a chain below the existing blocks. The chain is
    /// linked to the start block only when nothing else is in the graph.
    pub fn append_chain(&mut self, steps: &[Step]) {
        let empty = self.block_count() == 0;
        let start = self.start().map(|at| self.nodes[at].id.clone());
        let bottom = self.nodes.iter().filter_map(|node| node.y).reduce(f64::max);
        let left = self.nodes.iter().filter_map(|node| node.x).reduce(f64::min).unwrap_or(0.0);
        let mut y = bottom.map_or(0.0, |bottom| bottom + ROW);
        let mut previous = if empty { start } else { None };
        for step in steps {
            let Some(kind) = NodeKind::from_step(&step.kind) else {
                continue;
            };
            let mut node = Node::new(kind);
            node.enabled = step.enabled;
            node.pause_after = step.pause_after;
            node.repeat = step.repeat;
            node.x = Some(left);
            node.y = Some(y);
            y += ROW;
            if let Some(from) = previous.take() {
                self.edges.push(Edge { from, port: Port::Next, to: node.id.clone(), points: Vec::new() });
            }
            previous = Some(node.id.clone());
            self.nodes.push(node);
        }
    }
}

/// An output that still has to be linked to whatever comes next.
type Open = (String, Port);

/// What a run of steps turned into.
struct Piece {
    /// The first block the incoming outputs were linked to.
    entry: Option<String>,
    open: Vec<Open>,
}

struct Builder {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

impl Builder {
    fn add(&mut self, node: Node) -> String {
        let id = node.id.clone();
        self.nodes.push(node);
        id
    }

    fn link(&mut self, open: &mut Vec<Open>, to: &str) {
        for (from, port) in open.drain(..) {
            self.edges.push(Edge { from, port, to: to.to_string(), points: Vec::new() });
        }
    }

    fn pause_node(&mut self, duration: Pause) -> String {
        self.add(Node::new(NodeKind::Pause { duration }))
    }

    /// `breaks` collects the outputs that leave the nearest loop.
    fn chain(&mut self, steps: &[Step], mut open: Vec<Open>, breaks: &mut Vec<Open>) -> Piece {
        let mut entry: Option<String> = None;
        for step in steps {
            let is_block = matches!(step.kind, StepKind::If { .. } | StepKind::Loop { .. });
            if is_block && !step.enabled {
                // A switched-off block is carried over as a group nothing leads into.
                let mut unused = Vec::new();
                self.block(step, Vec::new(), &mut unused);
                continue;
            }
            if matches!(step.kind, StepKind::BreakLoop) {
                if step.enabled {
                    breaks.append(&mut open);
                }
                continue;
            }
            let piece = if is_block { self.block(step, open, breaks) } else { self.simple(step, open) };
            if entry.is_none() {
                entry = piece.entry;
            }
            open = piece.open;
        }
        Piece { entry, open }
    }

    fn simple(&mut self, step: &Step, mut open: Vec<Open>) -> Piece {
        let Some(kind) = NodeKind::from_step(&step.kind) else {
            return Piece { entry: None, open };
        };
        let mut node = Node::new(kind);
        if !step.id.is_empty() {
            node.id = step.id.clone();
        }
        node.enabled = step.enabled;
        node.pause_after = step.pause_after;
        node.repeat = step.repeat;
        let id = self.add(node);
        self.link(&mut open, &id);
        let open = match &step.kind {
            // Nothing follows an enabled "Stop"; a switched-off one is passed through.
            StepKind::Stop if step.enabled => Vec::new(),
            StepKind::Wait { on_timeout: OnTimeout::Continue, .. } => {
                vec![(id.clone(), Port::Next), (id.clone(), Port::Timeout)]
            }
            _ => vec![(id.clone(), Port::Next)],
        };
        Piece { entry: Some(id), open }
    }

    fn block(&mut self, step: &Step, mut open: Vec<Open>, breaks: &mut Vec<Open>) -> Piece {
        let named = |kind: NodeKind| {
            let mut node = Node::new(kind);
            if !step.id.is_empty() {
                node.id = step.id.clone();
            }
            node
        };
        let mut piece = match &step.kind {
            StepKind::If { condition, poll_ms, timeout_ms, then_steps, else_steps } => {
                let id = self.add(named(NodeKind::Branch {
                    condition: condition.clone(),
                    poll_ms: *poll_ms,
                    timeout_ms: *timeout_ms,
                }));
                self.link(&mut open, &id);
                let mut exits = self.chain(then_steps, vec![(id.clone(), Port::Yes)], breaks).open;
                exits.extend(self.chain(else_steps, vec![(id.clone(), Port::No)], breaks).open);
                Piece { entry: Some(id), open: exits }
            }
            StepKind::Loop { mode: LoopMode::Count { times }, pause_between, steps } => {
                let id = self.add(named(NodeKind::Counter { times: *times, pause_between: *pause_between }));
                self.link(&mut open, &id);
                let mut left = Vec::new();
                let mut body = self.chain(steps, vec![(id.clone(), Port::Body)], &mut left).open;
                self.link(&mut body, &id);
                left.insert(0, (id.clone(), Port::Done));
                Piece { entry: Some(id), open: left }
            }
            StepKind::Loop { mode: LoopMode::While { condition }, pause_between, steps } => {
                let id = self.add(named(NodeKind::Branch {
                    condition: condition.clone(),
                    poll_ms: DEFAULT_POLL_MS,
                    timeout_ms: 0,
                }));
                self.link(&mut open, &id);
                let mut left = Vec::new();
                let mut body = self.chain(steps, vec![(id.clone(), Port::Yes)], &mut left).open;
                if !pause_between.is_zero() && !body.is_empty() {
                    let pause = self.pause_node(*pause_between);
                    self.link(&mut body, &pause);
                    body.push((pause, Port::Next));
                }
                self.link(&mut body, &id);
                left.insert(0, (id.clone(), Port::No));
                Piece { entry: Some(id), open: left }
            }
            StepKind::Loop { mode: LoopMode::Forever, pause_between, steps } => {
                let mut left = Vec::new();
                let body = self.chain(steps, open, &mut left);
                let mut tail = body.open;
                let entry = match body.entry {
                    Some(entry) => {
                        if !pause_between.is_zero() && !tail.is_empty() {
                            let pause = self.pause_node(*pause_between);
                            self.link(&mut tail, &pause);
                            tail.push((pause, Port::Next));
                        }
                        self.link(&mut tail, &entry);
                        Some(entry)
                    }
                    // An endless loop with nothing in it: a pause that leads to itself.
                    None if !tail.is_empty() => {
                        let pause = self.pause_node(*pause_between);
                        self.link(&mut tail, &pause);
                        self.edges.push(Edge {
                            from: pause.clone(),
                            port: Port::Next,
                            to: pause.clone(),
                            points: Vec::new(),
                        });
                        Some(pause)
                    }
                    None => None,
                };
                Piece { entry, open: left }
            }
            _ => Piece { entry: None, open },
        };
        // The pause after a block becomes a block of its own where the exits meet.
        if !step.pause_after.is_zero() && !piece.open.is_empty() {
            let pause = self.pause_node(step.pause_after);
            self.link(&mut piece.open, &pause);
            piece.open.push((pause, Port::Next));
        }
        piece
    }
}

/// Turns a list of steps into a graph that runs the same way.
///
/// "If" becomes a condition block, "Repeat N times" a counter, "While" a
/// condition with a link back, "Forever" a link back, "Exit loop" a link to
/// what follows the loop.
pub fn from_steps(steps: &[Step]) -> Graph {
    let start = Node::new(NodeKind::Start);
    let first = vec![(start.id.clone(), Port::Next)];
    let mut builder = Builder { nodes: vec![start], edges: Vec::new() };
    // What is left open at the end, breaks outside a loop included, ends the pass.
    let mut stray = Vec::new();
    builder.chain(steps, first, &mut stray);
    let mut graph = Graph { nodes: builder.nodes, edges: builder.edges, notes: Vec::new() };
    graph.arrange();
    graph
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::scenario::Area;

    fn click() -> Step {
        Step::new(StepKind::Click {
            button: MouseButton::Left,
            action: ClickAction::Click,
            position: Position::Cursor,
        })
    }

    fn process(name: &str) -> Condition {
        Condition::Process { name: name.to_string() }
    }

    fn wait(on_timeout: OnTimeout) -> Step {
        Step::new(StepKind::Wait {
            condition: process("a.exe"),
            poll_ms: 200,
            timeout_ms: 1000,
            on_timeout,
            on_found: OnFound::None,
        })
    }

    fn looped(mode: LoopMode, steps: Vec<Step>) -> Step {
        Step::new(StepKind::Loop { mode, pause_between: Pause::default(), steps })
    }

    fn index(graph: &Graph, id: &str) -> usize {
        graph.wiring().index_of(id).unwrap()
    }

    /// Follows `ports` from the start block and returns where the walk ends.
    fn walk(graph: &Graph, ports: &[Port]) -> Follow {
        let wiring = graph.wiring();
        let mut at = Follow::To(graph.start().unwrap());
        for port in ports {
            let Follow::To(from) = at else {
                return at;
            };
            at = wiring.follow(from, *port);
        }
        at
    }

    #[test]
    fn a_graph_round_trips_through_json() {
        let json = r#"{
            "nodes": [
                { "id": "n1", "type": "start", "x": 0, "y": 0 },
                { "id": "n2", "type": "wait", "title": "Wait", "x": 0, "y": 140,
                  "condition": { "kind": "process", "name": "a.exe" }, "pollMs": 200, "timeoutMs": 4000 },
                { "id": "n3", "type": "counter", "times": 12 },
                { "id": "n4", "type": "call", "scenarioId": "sc1" },
                { "id": "n5", "type": "intercept", "condition": { "kind": "process", "name": "b.exe" } }
            ],
            "edges": [ { "from": "n1", "port": "next", "to": "n2" } ],
            "notes": [ { "id": "t1", "kind": "frame", "x": -40, "y": 100, "width": 700, "height": 220, "text": "A" } ]
        }"#;
        let graph: Graph = serde_json::from_str(json).unwrap();
        assert_eq!(graph.nodes[1].title, "Wait");
        assert_eq!(graph.nodes[2].x, None);
        assert!(matches!(graph.nodes[4].kind, NodeKind::Intercept { min_interval_ms: DEFAULT_INTERCEPT_MS, .. }));
        assert_eq!(graph.notes[0].kind, NoteKind::Frame);
        assert_eq!(graph.callees(), vec!["sc1".to_string()]);
        let back: Graph = serde_json::from_str(&serde_json::to_string(&graph).unwrap()).unwrap();
        assert_eq!(back, graph);
        let value = serde_json::to_value(&graph).unwrap();
        assert_eq!(value["nodes"][1]["type"], "wait");
        assert!(value["nodes"][2].get("x").is_none());
    }

    #[test]
    fn an_unlinked_output_ends_the_pass_and_an_unlinked_timeout_fails() {
        let graph = from_steps(&[wait(OnTimeout::Stop), click()]);
        assert!(matches!(walk(&graph, &[Port::Next, Port::Next]), Follow::To(_)));
        assert_eq!(walk(&graph, &[Port::Next, Port::Next, Port::Next]), Follow::End);
        assert_eq!(walk(&graph, &[Port::Next, Port::Timeout]), Follow::Timeout);
    }

    #[test]
    fn a_wait_that_goes_on_after_a_timeout_links_both_outputs() {
        let graph = from_steps(&[wait(OnTimeout::Continue), click()]);
        assert_eq!(walk(&graph, &[Port::Next, Port::Next]), walk(&graph, &[Port::Next, Port::Timeout]));
        assert!(matches!(walk(&graph, &[Port::Next, Port::Timeout]), Follow::To(_)));
    }

    #[test]
    fn a_counter_goes_into_the_body_the_given_number_of_times() {
        assert_eq!(counter_port(2, 0), Port::Body);
        assert_eq!(counter_port(2, 1), Port::Body);
        assert_eq!(counter_port(2, 2), Port::Done);
        assert_eq!(counter_port(0, 0), Port::Done);
    }

    #[test]
    fn an_if_block_becomes_a_condition_whose_branches_meet_again() {
        let branch = Step::new(StepKind::If {
            condition: process("a.exe"),
            poll_ms: 200,
            timeout_ms: 0,
            then_steps: vec![click()],
            else_steps: vec![],
        });
        let last = click();
        let last_id = last.id.clone();
        let graph = from_steps(&[branch, last]);
        assert!(matches!(graph.nodes[1].kind, NodeKind::Branch { .. }));
        let after = Follow::To(index(&graph, &last_id));
        assert_eq!(walk(&graph, &[Port::Next, Port::Yes, Port::Next]), after);
        assert_eq!(walk(&graph, &[Port::Next, Port::No]), after);
    }

    #[test]
    fn a_counted_loop_becomes_a_counter_with_a_link_back() {
        let body = click();
        let body_id = body.id.clone();
        let mut repeat = looped(LoopMode::Count { times: 3 }, vec![body]);
        repeat.pause_after = Pause::exact(500);
        let graph = from_steps(&[repeat]);
        let counter = graph.nodes.iter().position(|node| matches!(node.kind, NodeKind::Counter { times: 3, .. }));
        let counter = counter.unwrap();
        assert_eq!(walk(&graph, &[Port::Next, Port::Body]), Follow::To(index(&graph, &body_id)));
        assert_eq!(walk(&graph, &[Port::Next, Port::Body, Port::Next]), Follow::To(counter));
        // The pause after the block is a block of its own on the way out.
        let Follow::To(pause) = walk(&graph, &[Port::Next, Port::Done]) else { panic!() };
        assert_eq!(graph.nodes[pause].kind, NodeKind::Pause { duration: Pause::exact(500) });
    }

    #[test]
    fn a_while_loop_becomes_a_condition_with_a_link_back() {
        let graph = from_steps(&[looped(LoopMode::While { condition: process("a.exe") }, vec![click()]), click()]);
        let Follow::To(branch) = walk(&graph, &[Port::Next]) else { panic!() };
        assert!(matches!(graph.nodes[branch].kind, NodeKind::Branch { timeout_ms: 0, .. }));
        assert_eq!(walk(&graph, &[Port::Next, Port::Yes, Port::Next]), Follow::To(branch));
        assert!(matches!(walk(&graph, &[Port::Next, Port::No]), Follow::To(_)));
    }

    #[test]
    fn an_endless_loop_links_its_end_to_its_beginning() {
        let first = click();
        let first_id = first.id.clone();
        let graph = from_steps(&[looped(LoopMode::Forever, vec![first, click()])]);
        let entry = Follow::To(index(&graph, &first_id));
        assert_eq!(walk(&graph, &[Port::Next]), entry);
        assert_eq!(walk(&graph, &[Port::Next, Port::Next, Port::Next]), entry);
    }

    #[test]
    fn an_exit_from_a_loop_leads_to_what_follows_it() {
        let leave = Step::new(StepKind::If {
            condition: process("a.exe"),
            poll_ms: 200,
            timeout_ms: 0,
            then_steps: vec![Step::new(StepKind::BreakLoop)],
            else_steps: vec![],
        });
        let after = click();
        let after_id = after.id.clone();
        let graph = from_steps(&[looped(LoopMode::Forever, vec![leave]), after]);
        assert_eq!(walk(&graph, &[Port::Next, Port::Yes]), Follow::To(index(&graph, &after_id)));
        // "No" goes round again.
        assert_eq!(walk(&graph, &[Port::Next, Port::No]), walk(&graph, &[Port::Next]));
    }

    #[test]
    fn a_switched_off_block_is_carried_over_without_links_into_it() {
        let inner = click();
        let inner_id = inner.id.clone();
        let mut off = looped(LoopMode::Count { times: 2 }, vec![inner]);
        off.enabled = false;
        let off_id = off.id.clone();
        let mut simple = click();
        simple.enabled = false;
        let simple_id = simple.id.clone();
        let graph = from_steps(&[off, simple]);
        let wiring = graph.wiring();
        let reached = graph.reachable(&wiring, graph.start().unwrap());
        assert!(!reached.contains(&index(&graph, &off_id)));
        assert!(!reached.contains(&index(&graph, &inner_id)));
        // A switched-off simple step stays in the chain and is passed through.
        assert!(reached.contains(&index(&graph, &simple_id)));
        assert!(!graph.node(&simple_id).unwrap().enabled);
    }

    #[test]
    fn simple_steps_keep_their_fields_and_ids() {
        let mut step = click();
        step.repeat = Some(Repeat { times: 5, pause_between: Pause::exact(10) });
        step.pause_after = Pause::exact(30);
        let graph = from_steps(&[step.clone(), Step::new(StepKind::Stop), click()]);
        let node = graph.node(&step.id).unwrap();
        assert_eq!(node.repeat, step.repeat);
        assert_eq!(node.pause_after, step.pause_after);
        assert_eq!(node.kind.action(), Some(step.kind));
        // Nothing is linked after "Stop".
        assert_eq!(walk(&graph, &[Port::Next, Port::Next, Port::Next]), Follow::End);
        assert_eq!(graph.block_count(), 3);
    }

    #[test]
    fn the_layout_puts_branches_into_columns_of_their_own() {
        let branch = Step::new(StepKind::If {
            condition: process("a.exe"),
            poll_ms: 200,
            timeout_ms: 0,
            then_steps: vec![click()],
            else_steps: vec![click()],
        });
        let graph = from_steps(&[branch]);
        let mut seen = HashSet::new();
        for node in &graph.nodes {
            let spot = (node.x.unwrap() as i64, node.y.unwrap() as i64);
            assert!(seen.insert(spot), "two blocks share a place");
        }
        assert_eq!(graph.nodes[0].y, Some(0.0));
        assert_eq!(graph.nodes[1].y, Some(ROW));
    }

    #[test]
    fn only_blocks_without_a_position_are_placed() {
        let mut graph = Graph::new();
        let mut kept = Node::new(NodeKind::Stop);
        kept.x = Some(500.0);
        kept.y = Some(320.0);
        let added = Node::new(NodeKind::Text { text: "a".into() });
        let added_id = added.id.clone();
        graph.edges.push(Edge { from: graph.nodes[0].id.clone(), port: Port::Next, to: added_id.clone(), points: vec![] });
        graph.nodes.push(kept.clone());
        graph.nodes.push(added);
        graph.place_missing();
        assert_eq!(graph.nodes[1], kept);
        let placed = graph.node(&added_id).unwrap();
        assert!(placed.y.unwrap() > 320.0);
        assert!(placed.x.is_some());
    }

    #[test]
    fn fresh_ids_keep_the_links_and_templates_are_found() {
        let image = Condition::ImageFound { template: "a.png".into(), area: Area::Screen, threshold: 0.9 };
        let mut graph = from_steps(&[Step::new(StepKind::Wait {
            condition: image,
            poll_ms: 200,
            timeout_ms: 0,
            on_timeout: OnTimeout::Stop,
            on_found: OnFound::None,
        })]);
        let before: Vec<String> = graph.nodes.iter().map(|node| node.id.clone()).collect();
        graph.renew_ids();
        assert!(graph.nodes.iter().all(|node| !before.contains(&node.id)));
        assert!(matches!(walk(&graph, &[Port::Next]), Follow::To(1)));
        let mut templates = Vec::new();
        graph.collect_templates(&mut templates);
        assert_eq!(templates, vec!["a.png".to_string()]);
    }

    #[test]
    fn chosen_blocks_move_out_and_a_call_block_takes_their_place() {
        let steps = [click(), click(), click(), click()];
        let ids: Vec<String> = steps.iter().map(|step| step.id.clone()).collect();
        let mut graph = from_steps(&steps);
        let taken = graph.extract(&ids[1..3], "sc-new", "Middle").unwrap();

        // The rest: start, the first click, the call block, the last click.
        assert_eq!(graph.block_count(), 3);
        let Follow::To(call) = walk(&graph, &[Port::Next, Port::Next]) else { panic!() };
        assert_eq!(graph.nodes[call].kind, NodeKind::Call { scenario_id: "sc-new".into() });
        assert_eq!(graph.nodes[call].title, "Middle");
        assert_eq!(walk(&graph, &[Port::Next, Port::Next, Port::Next]), Follow::To(index(&graph, &ids[3])));

        // The new graph: its own start and the two clicks, ending the pass after the second.
        assert_eq!(taken.block_count(), 2);
        assert_eq!(walk(&taken, &[Port::Next]), Follow::To(index(&taken, &ids[1])));
        assert_eq!(walk(&taken, &[Port::Next, Port::Next]), Follow::To(index(&taken, &ids[2])));
        assert_eq!(walk(&taken, &[Port::Next, Port::Next, Port::Next]), Follow::End);
    }

    #[test]
    fn blocks_with_two_ways_in_or_out_are_not_moved_out() {
        let branch = Step::new(StepKind::If {
            condition: process("a.exe"),
            poll_ms: 200,
            timeout_ms: 0,
            then_steps: vec![click()],
            else_steps: vec![click()],
        });
        let StepKind::If { then_steps, else_steps, .. } = &branch.kind else { unreachable!() };
        let both = vec![then_steps[0].id.clone(), else_steps[0].id.clone()];
        let branch_id = branch.id.clone();
        let last = click();
        let mut graph = from_steps(&[branch, last]);
        let before = graph.clone();
        // Two blocks the pass enters separately.
        assert_eq!(graph.extract(&both, "x", "x").unwrap_err(), "extractManyEntries");
        // The condition alone leaves through two outputs to two different blocks.
        assert_eq!(graph.extract(std::slice::from_ref(&branch_id), "x", "x").unwrap_err(), "extractManyExits");
        assert_eq!(graph.extract(&[graph.nodes[0].id.clone()], "x", "x").unwrap_err(), "extractEntry");
        assert_eq!(graph.extract(&["nope".to_string()], "x", "x").unwrap_err(), "extractEmpty");
        assert_eq!(graph, before);
        // The condition with both of its branches has one way in and one way out.
        let mut whole = both;
        whole.push(branch_id);
        assert!(graph.extract(&whole, "x", "x").is_ok());
        assert_eq!(graph.block_count(), 2);
    }

    #[test]
    fn recorded_steps_join_the_start_only_in_an_empty_graph() {
        let mut graph = Graph::new();
        graph.append_chain(&[click(), click()]);
        assert!(matches!(walk(&graph, &[Port::Next, Port::Next]), Follow::To(2)));
        let links = graph.edges.len();
        graph.append_chain(&[click(), click()]);
        // The second chain is linked inside, not to the start.
        assert_eq!(graph.edges.len(), links + 1);
        assert_eq!(walk(&graph, &[Port::Next, Port::Next, Port::Next]), Follow::End);
        assert!(graph.nodes[3].y.unwrap() > graph.nodes[2].y.unwrap());
    }
}
