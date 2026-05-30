import type {
  CheckConfig,
  DirectorySequenceConfig,
  LinkRuleConfig,
  NumberedDocumentConfig,
  SectionReferenceConfig,
} from "./config.ts";
import {
  DEFAULT_DISPLAYED_MARKDOWN_PATH_PATTERN,
  DEFAULT_EXTERNAL_PREFIXES,
  isEnabled,
} from "./config.ts";
import { DiagnosticBag } from "./diagnostics.ts";
import type { DocumentTree } from "./document-tree.ts";
import {
  basename,
  dirname,
  isFile,
  relativePath,
  resolveAgainst,
} from "./document-tree.ts";
import { deno } from "./deno-runtime.ts";

interface LinkRef {
  text: string;
  target: string;
}

interface DirectorySequenceEntry {
  number: number;
  numberText: string;
}

type Rule = (context: RuleContext) => void | Promise<void>;

interface RuleContext {
  config: CheckConfig;
  tree: DocumentTree;
  diagnostics: DiagnosticBag;
}

const MARKDOWN_LINK_RE = /\[([^\]]*)\]\(([^)]+)\)/g;

export async function runConfiguredRules(
  config: CheckConfig,
  tree: DocumentTree,
  diagnostics: DiagnosticBag,
): Promise<void> {
  const rules: Rule[] = [];

  const linkRule = config.links;
  if (isEnabled(linkRule)) {
    rules.push((context) => checkLinks(context, linkRule));
  }
  const numberedRule = config.numberedDocuments;
  if (isEnabled(numberedRule)) {
    rules.push((context) => checkNumberedHeadings(context, numberedRule));
    rules.push((context) => checkBareNumberedReferences(context, numberedRule));
  }
  for (const rule of config.directorySequences ?? []) {
    if (!isEnabled(rule)) continue;
    rules.push((context) => checkDirectorySequences(context, rule));
  }
  for (const rule of config.sectionReferences ?? []) {
    if (!isEnabled(rule)) continue;
    rules.push((context) => checkSectionReferences(context, rule));
  }

  const context: RuleContext = { config, tree, diagnostics };
  for (const rule of rules) {
    await rule(context);
  }
}

function checkLinks(
  { tree, diagnostics }: RuleContext,
  rule: LinkRuleConfig,
): void {
  const externalPrefixes = rule.externalPrefixes ?? DEFAULT_EXTERNAL_PREFIXES;
  const displayedPathPattern = compileRegExp(
    rule.displayedMarkdownPathPattern,
    DEFAULT_DISPLAYED_MARKDOWN_PATH_PATTERN,
  );

  for (const file of tree.files) {
    for (const link of markdownLinks(file.text)) {
      const target = link.target.trim().split(/\s+/, 1)[0];
      if (!target || isExternalTarget(target, externalPrefixes)) continue;

      const targetPath = target.split("#", 1)[0];
      if (!targetPath || !isDocumentLink(targetPath, tree.extensions)) continue;

      const resolvedPath = resolveAgainst(dirname(file.path), targetPath);
      if (!tree.filePaths.has(resolvedPath)) {
        diagnostics.error(
          `broken markdown link: ${file.relativePath} -> ${target}`,
        );
        continue;
      }

      if (rule.checkDisplayedMarkdownFilename === false) continue;

      const displayedTarget = stripBackticks(link.text.trim());
      if (!displayedPathPattern.test(displayedTarget)) continue;

      const displayedBasename = basename(displayedTarget);
      const targetBasename = basename(targetPath);
      if (displayedBasename !== targetBasename) {
        diagnostics.error(
          `markdown link text does not match target filename: ${file.relativePath} has [${link.text}] -> ${targetBasename}`,
        );
      }
    }
  }
}

function checkNumberedHeadings(
  { tree, diagnostics }: RuleContext,
  rule: NumberedDocumentConfig,
): void {
  const filenamePattern = new RegExp(rule.filenamePattern);
  const headingPattern = new RegExp(rule.headingPattern);

  for (const file of tree.files) {
    const filenameNumber = extractGroup(
      file.basename.match(filenamePattern),
      "number",
    );
    if (!filenameNumber) continue;

    const firstHeading = firstH1(file.text);
    if (!firstHeading) {
      diagnostics.error(`missing H1 heading: ${file.relativePath}`);
      continue;
    }

    const headingNumber = extractGroup(
      firstHeading.match(headingPattern),
      "number",
    );
    if (!headingNumber) {
      diagnostics.error(
        `H1 heading does not match configured numbered-heading pattern: ${file.relativePath}`,
      );
    } else if (headingNumber !== filenameNumber) {
      diagnostics.error(
        `filename/H1 number mismatch: ${file.relativePath} has H1 ${headingNumber}`,
      );
    }
  }
}

function checkBareNumberedReferences(
  { tree, diagnostics }: RuleContext,
  rule: NumberedDocumentConfig,
): void {
  if (!rule.referencePattern) return;

  const referencePattern = ensureGlobalRegExp(rule.referencePattern);
  for (const file of tree.files) {
    for (const match of file.text.matchAll(referencePattern)) {
      const filename = extractGroup(match, "filename");
      if (!filename || tree.fileBasenames.has(filename)) continue;
      diagnostics.error(
        `unknown numbered markdown filename reference in ${file.relativePath}: ${filename}`,
      );
    }
  }
}

function checkDirectorySequences(
  { config, tree, diagnostics }: RuleContext,
  rule: DirectorySequenceConfig,
): void {
  const filenamePatternSource =
    rule.filenamePattern ?? config.numberedDocuments?.filenamePattern;
  if (!filenamePatternSource) {
    diagnostics.error(
      "directory sequence rule requires filenamePattern or numberedDocuments.filenamePattern",
    );
    return;
  }

  const filenamePattern = new RegExp(filenamePatternSource);
  const includeDirs = compilePatterns(rule.includeDirs);
  const excludeDirs = compilePatterns(rule.excludeDirs);
  const directories = new Map<string, DirectorySequenceEntry[]>();

  for (const file of tree.files) {
    const numberText = extractGroup(
      file.basename.match(filenamePattern),
      "number",
    );
    if (
      !numberText ||
      !isIncludedDirectory(file.directory, includeDirs, excludeDirs)
    ) {
      continue;
    }

    const entries = directories.get(file.directory) ?? [];
    entries.push({ number: Number.parseInt(numberText, 10), numberText });
    directories.set(file.directory, entries);
  }

  const startAt = rule.startAt ?? 0;
  const minFiles = rule.minFiles ?? 1;
  for (const [directory, entries] of directories) {
    if (entries.length < minFiles) continue;

    entries.sort((left, right) => left.number - right.number);
    const actualNumbers = entries.map((entry) => entry.number);
    const expectedNumbers = entries.map((_, index) => index + startAt);
    if (sameNumberList(actualNumbers, expectedNumbers)) continue;

    const width = Math.max(
      2,
      ...entries.map((entry) => entry.numberText.length),
    );
    diagnostics.error(
      `numbered markdown files are not continuous in ${directory}: expected ${formatNumberList(
        expectedNumbers,
        width,
      )}, got ${formatNumberList(actualNumbers, width)}`,
    );
  }
}

async function checkSectionReferences(
  { tree, diagnostics }: RuleContext,
  rule: SectionReferenceConfig,
): Promise<void> {
  const targetFile = resolveAgainst(tree.documentRoot, rule.targetFile);
  const sections = await readSections(
    targetFile,
    tree.projectRoot,
    rule,
    diagnostics,
  );
  const refPattern = ensureGlobalRegExp(rule.sectionRefPattern);

  for (const file of tree.files) {
    const lines = file.text.split("\n");
    lines.forEach((line, index) => {
      if (!lineMatchesIncludes(line, rule.lineIncludes)) return;
      for (const match of line.matchAll(refPattern)) {
        const section = extractGroup(match, "section");
        if (!section || sections.has(section)) continue;
        diagnostics.error(
          `stale ${rule.label} section reference in ${file.relativePath}:${
            index + 1
          }: §${section}`,
        );
      }
    });
  }
}

async function readSections(
  targetFile: string,
  projectRoot: string,
  rule: SectionReferenceConfig,
  diagnostics: DiagnosticBag,
): Promise<Set<string>> {
  if (!(await isFile(targetFile))) {
    diagnostics.error(
      `missing ${rule.label} section target: ${relativePath(
        projectRoot,
        targetFile,
      )}`,
    );
    return new Set();
  }

  const sectionPattern = new RegExp(rule.sectionHeadingPattern);
  const sections = new Set<string>();
  const text = await deno.readTextFile(targetFile);
  for (const line of text.split("\n")) {
    const section = extractGroup(line.match(sectionPattern), "section");
    if (section) sections.add(section);
  }
  return sections;
}

function markdownLinks(text: string): LinkRef[] {
  return [...text.matchAll(MARKDOWN_LINK_RE)].map((match) => ({
    text: match[1],
    target: match[2],
  }));
}

function firstH1(text: string): string | undefined {
  return text.split("\n").find((line) => line.startsWith("# "));
}

function isDocumentLink(targetPath: string, extensions: string[]): boolean {
  return extensions.some((extension) => targetPath.endsWith(extension));
}

function compileRegExp(pattern: string | undefined, fallback: RegExp): RegExp {
  return pattern ? new RegExp(pattern) : fallback;
}

function ensureGlobalRegExp(pattern: string): RegExp {
  return new RegExp(pattern, "g");
}

function extractGroup(
  match: RegExpMatchArray | null,
  groupName: string,
): string | undefined {
  if (!match) return undefined;
  return match.groups?.[groupName] ?? match[1];
}

function lineMatchesIncludes(
  line: string,
  includes: string[] | undefined,
): boolean {
  return !includes || includes.every((token) => line.includes(token));
}

function isExternalTarget(target: string, prefixes: string[]): boolean {
  return prefixes.some((prefix) => target.startsWith(prefix));
}

function stripBackticks(text: string): string {
  return text.startsWith("`") && text.endsWith("`") ? text.slice(1, -1) : text;
}

function compilePatterns(patterns: string[] | undefined): RegExp[] {
  return patterns?.map((pattern) => new RegExp(pattern)) ?? [];
}

function isIncludedDirectory(
  directory: string,
  includeDirs: RegExp[],
  excludeDirs: RegExp[],
): boolean {
  return (
    (includeDirs.length === 0 ||
      includeDirs.some((pattern) => pattern.test(directory))) &&
    !excludeDirs.some((pattern) => pattern.test(directory))
  );
}

function sameNumberList(left: number[], right: number[]): boolean {
  return (
    left.length === right.length &&
    left.every((value, index) => value === right[index])
  );
}

function formatNumberList(numbers: number[], width: number): string {
  return numbers
    .map((number) => number.toString().padStart(width, "0"))
    .join(", ");
}
