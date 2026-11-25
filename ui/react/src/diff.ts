// A unified patch as rows to draw: per file, with line numbers and long unchanged parts folded.

export type DiffRow =
  | { kind: "add" | "remove" | "context"; text: string; old?: number; new?: number }
  | { kind: "hunk"; text: string }
  /** Unchanged lines that are not shown until the user asks. */
  | { kind: "fold"; rows: DiffRow[] };

export interface DiffFile {
  path: string;
  added: number;
  removed: number;
  rows: DiffRow[];
}

const name = (line: string) => line.slice(4).replace(/\t.*$/, "").replace(/^[ab]\//, "");

/** Reads a unified patch (what `git diff` prints). Text that is no patch gives no files. */
export function parsePatch(patch: string): DiffFile[] {
  const files: DiffFile[] = [];
  let file: DiffFile | null = null;
  let oldLine = 0, newLine = 0, inHunk = false;
  const lines = patch.split("\n");
  if (lines[lines.length - 1] === "") lines.pop();
  for (let at = 0; at < lines.length; at += 1) {
    const line = lines[at]!;
    if (line.startsWith("diff --git ")) { file = null; inHunk = false; continue; }
    if (!inHunk || line.startsWith("--- ")) {
      if (line.startsWith("--- ") && lines[at + 1]?.startsWith("+++ ")) {
        const to = name(lines[at + 1]!);
        file = { path: to === "/dev/null" || to === "dev/null" ? name(line) : to, added: 0, removed: 0, rows: [] };
        files.push(file);
        inHunk = false;
        at += 1;
        continue;
      }
    }
    const hunk = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(line);
    if (hunk) {
      if (!file) { file = { path: "", added: 0, removed: 0, rows: [] }; files.push(file); }
      oldLine = Number(hunk[1]);
      newLine = Number(hunk[2]);
      inHunk = true;
      file.rows.push({ kind: "hunk", text: line });
      continue;
    }
    if (!file || !inHunk) continue; // index lines, modes, and text before the first hunk
    if (line.startsWith("+")) { file.rows.push({ kind: "add", text: line.slice(1), new: newLine }); newLine += 1; file.added += 1; }
    else if (line.startsWith("-")) { file.rows.push({ kind: "remove", text: line.slice(1), old: oldLine }); oldLine += 1; file.removed += 1; }
    else if (line.startsWith("\\")) continue; // "\ No newline at end of file"
    else { file.rows.push({ kind: "context", text: line.slice(1), old: oldLine, new: newLine }); oldLine += 1; newLine += 1; }
  }
  return files;
}

/**
 * Folds runs of unchanged lines, keeping `keep` of them next to each change. A run is folded
 * only when that hides more than three lines; fewer are not worth a click.
 */
export function foldContext(rows: DiffRow[], keep = 3): DiffRow[] {
  const folded: DiffRow[] = [];
  for (let at = 0; at < rows.length;) {
    if (rows[at]!.kind !== "context") { folded.push(rows[at]!); at += 1; continue; }
    let end = at;
    while (end < rows.length && rows[end]!.kind === "context") end += 1;
    const run = rows.slice(at, end);
    // No change before the run (the start, or a hunk header) or after it: keep that side short.
    const before = folded.length > 0 && folded[folded.length - 1]!.kind !== "hunk" ? keep : 0;
    const after = end < rows.length ? keep : 0;
    if (run.length - before - after > 3) {
      folded.push(...run.slice(0, before), { kind: "fold", rows: run.slice(before, run.length - after) }, ...run.slice(run.length - after));
    } else {
      folded.push(...run);
    }
    at = end;
  }
  return folded;
}
