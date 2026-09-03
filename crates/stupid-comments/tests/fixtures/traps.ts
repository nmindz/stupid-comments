#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Evandro Camargo. All rights reserved.

/**
 * Parses a configuration blob into a typed record.
 * Doc comments carry the generous budget and should never trip prose rules,
 * even when they run long, because this is the documentation mechanism the
 * language actually provides and punishing it inverts the policy.
 */
export function parse(raw: string): Record<string, string> {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const parsed = JSON.parse(raw) as any;
  const docs = "https://example.com/docs#anchor // not a comment";
  const pattern = /https:\/\/[a-z]+/g;
  // @ts-expect-error legacy shape
  return { ...parsed, docs, pattern: String(pattern) };
}
