const REFERENCE_DEF_RE = /^[ ]{0,3}\[[^\]]+\]:/mu;
const LIST_ITEM_RE = /^[ ]{0,3}(?:[-*+]|\d{1,9}[.)])[ \t]+/u;

function startsWithListItem(blockLines: readonly string[]): boolean {
  for (const line of blockLines) {
    if (line.trim().length === 0) continue;
    return LIST_ITEM_RE.test(line);
  }
  return false;
}

/**
 * Splits Markdown into top-level logical blocks so streaming renders can
 * memoize completed paragraphs, tables, lists, and fenced code blocks instead
 * of re-parsing the entire message AST on every token delta.
 *
 * Documents containing CommonMark reference link definitions or footnotes stay
 * intact as a single block so cross-block references always resolve.
 */
export function splitMarkdownBlocks(markdown: string): string[] {
  if (!markdown) return [];
  if (REFERENCE_DEF_RE.test(markdown)) {
    return [markdown];
  }

  const lines = markdown.split("\n");
  const rawBlocks: string[][] = [];
  let current: string[] = [];
  let fenceChar: "`" | "~" | null = null;
  let fenceLength = 0;

  const flush = (): void => {
    if (current.length === 0) return;
    if (current.some((l) => l.trim().length > 0)) {
      rawBlocks.push(current);
    }
    current = [];
  };

  for (const line of lines) {
    const trimmed = line.trimStart();
    if (fenceChar !== null) {
      current.push(line);
      const closeMatch =
        fenceChar === "`"
          ? /^(`{3,})[ \t]*$/u.exec(trimmed)
          : /^(~{3,})[ \t]*$/u.exec(trimmed);
      if (closeMatch && closeMatch[1]!.length >= fenceLength) {
        fenceChar = null;
        fenceLength = 0;
      }
      continue;
    }

    const openMatch = /^(`{3,}|~{3,})/u.exec(trimmed);
    if (openMatch) {
      const marker = openMatch[1]!;
      const ch = marker[0] as "`" | "~";
      if (ch === "~" || !trimmed.slice(marker.length).includes("`")) {
        if (current.length > 0 && current[current.length - 1]!.trim() === "") {
          flush();
        }
        fenceChar = ch;
        fenceLength = marker.length;
        current.push(line);
        continue;
      }
    }

    if (trimmed.length === 0) {
      flush();
      continue;
    }

    current.push(line);
  }
  flush();

  // Merge consecutive loose list blocks so ordered lists keep numbering across blank lines.
  const merged: string[] = [];
  let prevWasList = false;
  for (const blockLines of rawBlocks) {
    const isList = startsWithListItem(blockLines);
    const text = blockLines.join("\n");
    if (isList && prevWasList && merged.length > 0) {
      merged[merged.length - 1] = `${merged[merged.length - 1]}\n\n${text}`;
    } else {
      merged.push(text);
    }
    prevWasList = isList;
  }
  return merged;
}
