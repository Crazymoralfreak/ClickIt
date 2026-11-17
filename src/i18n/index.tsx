// Dictionaries and the translation hook. `ru.json` defines the key set;
// `en.json` must have exactly the same shape (checked by the compiler below).

import { createContext, useContext, useMemo, type ReactNode } from "react";
import type { AppError } from "../types/runtime";
import type { Language } from "../types/scenario";
import en from "./en.json";
import ru from "./ru.json";

type Dictionary = typeof ru;

type LeafKeys<T, Prefix extends string = ""> = T extends string
  ? Prefix
  : { [K in keyof T & string]: LeafKeys<T[K], Prefix extends "" ? K : `${Prefix}.${K}`> }[keyof T & string];

/** Every dotted path to a string in the dictionaries, e.g. `"common.save"`. */
export type TKey = LeafKeys<Dictionary>;
export type TParams = Record<string, string | number>;

const dictionaries: Record<Language, Dictionary> = { ru, en };

function lookup(dictionary: Dictionary, key: string): string | undefined {
  let node: unknown = dictionary;
  for (const part of key.split(".")) {
    if (typeof node !== "object" || node === null) {
      return undefined;
    }
    node = (node as Record<string, unknown>)[part];
  }
  return typeof node === "string" ? node : undefined;
}

function fill(template: string, params?: TParams): string {
  if (!params) {
    return template;
  }
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in params ? String(params[name]) : match,
  );
}

/** Looks a key up without type checking; returns `fallback` (or the key) when it is missing. */
export function translate(language: Language, key: string, params?: TParams, fallback?: string): string {
  const template = lookup(dictionaries[language], key);
  return template === undefined ? (fallback ?? key) : fill(template, params);
}

export interface Translator {
  language: Language;
  /** Static, compile-time checked key. */
  t: (key: TKey, params?: TParams) => string;
  /** Key built at run time, e.g. from an error code. */
  td: (key: string, params?: TParams, fallback?: string) => string;
  /** Text of an error returned by a command. */
  error: (error: AppError) => string;
}

function createTranslator(language: Language): Translator {
  const td = (key: string, params?: TParams, fallback?: string) => translate(language, key, params, fallback);
  return {
    language,
    t: (key, params) => translate(language, key, params),
    td,
    error: (error) => {
      const fallback = fill(translate(language, "errors.unknown"), { message: error.code });
      return td(`errors.${error.code}`, error.params, fallback);
    },
  };
}

const I18nContext = createContext<Translator>(createTranslator("en"));

export function I18nProvider({ language, children }: { language: Language; children: ReactNode }) {
  const translator = useMemo(() => createTranslator(language), [language]);
  return <I18nContext.Provider value={translator}>{children}</I18nContext.Provider>;
}

export function useT(): Translator {
  return useContext(I18nContext);
}
