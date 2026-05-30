import { walk } from "@std/fs/walk";
import {
  basename,
  dirname,
  extname,
  isAbsolute,
  normalize,
  relative,
  resolve,
} from "@std/path";
import { deno } from "./deno-runtime.ts";

export type Severity = "error" | "warning";

export interface Diagnostic {
  severity: Severity;
  message: string;
}

export interface CheckResult {
  errors: Diagnostic[];
  warnings: Diagnostic[];
}

export interface CheckConfig {
  projectRoot?: string;
  documents: DocumentConfig;
  links?: LinkRuleConfig;
  numberedDocuments?: NumberedDocumentConfig;
  directorySequences?: DirectorySequenceConfig[];
  sectionReferences?: SectionReferenceConfig[];
}

export interface DocumentConfig {
  root: string;
  extensions?: string[];
}

export interface LinkRuleConfig {
  enabled?: boolean;
  externalPrefixes?: string[];
  checkDisplayedMarkdownFilename?: boolean;
  displayedMarkdownPathPattern?: string;
}

export interface NumberedDocumentConfig {
  enabled?: boolean;
  filenamePattern: string;
  headingPattern: string;
  referencePattern?: string;
}

export interface DirectorySequenceConfig {
  enabled?: boolean;
  filenamePattern?: string;
  startAt?: number;
  minFiles?: number;
  includeDirs?: string[];
  excludeDirs?: string[];
}

export interface SectionReferenceConfig {
  enabled?: boolean;
  label: string;
  targetFile: string;
  lineIncludes?: string[];
  sectionHeadingPattern: string;
  sectionRefPattern: string;
}

interface MarkdownFile {
  path: string;
  relativePath: string;
  basename: string;
  text: string;
}

interface LinkRef {
  text: string;
  target: string;
}

interface DirectorySequenceEntry {
  number: number;
  numberText: string;
}

const MARKDOWN_LINK_RE = /\[([^\]]*)\]\(([^)]+)\)/g;
const DEFAULT_DOCUMENT_EXTENSIONS = [".md"];
const DEFAULT_EXTERNAL_PREFIXES = ["http://", "https://", "mailto:", "#"];
const DEFAULT_DISPLAYED_MARKDOWN_PATH_PATTERN =
  /^(?:\.\.?\/)?(?:[^/\]]+\/)*[^/\]]+\.md$/;

export async function loadConfig(configPath: string): Promise<CheckConfig> {
  const raw = JSON.parse(
    await deno.readTextFile(configPath),
  ) as Partial<CheckConfig>;
  return normalizeConfig(raw, configPath);
}

export async function checkDocs(
  projectRoot: string,
  config: CheckConfig,
): Promise<CheckResult> {
  const normalizedConfig = normalizeConfig(config, "inline config");
  const root = normalizePath(await deno.realPath(projectRoot));
  const errors: Diagnostic[] = [];
  const warnings: Diagnostic[] = [];
  const documentRoot = resolveAgainst(root, normalizedConfig.documents.root);
  const documentLabel = toSlash(normalizedConfig.documents.root);
  const extensions =
    normalizedConfig.documents.extensions ?? DEFAULT_DOCUMENT_EXTENSIONS;

  if (!(await isDirectory(documentRoot))) {
    errors.push(error(`document root not found: ${documentLabel}`));
    return { errors, warnings };
  }

  const markdownFiles = await readMarkdownFiles(documentRoot, root, extensions);
  const markdownBasenames = new Set(markdownFiles.map((file) => file.basename));
  const markdownPaths = new Set(markdownFiles.map((file) => file.path));

  if (isEnabled(normalizedConfig.links)) {
    checkLinks(
      markdownFiles,
      markdownPaths,
      extensions,
      normalizedConfig.links,
      errors,
    );
  }
  const numberedRule = normalizedConfig.numberedDocuments;
  if (isEnabled(numberedRule)) {
    checkNumberedHeadings(markdownFiles, numberedRule, errors);
    checkBareNumberedReferences(
      markdownFiles,
      markdownBasenames,
      numberedRule,
      errors,
    );
  }
  for (const rule of normalizedConfig.directorySequences ?? []) {
    if (!isEnabled(rule)) continue;
    checkDirectorySequences(markdownFiles, rule, normalizedConfig, errors);
  }
  for (const sectionRule of normalizedConfig.sectionReferences ?? []) {
    if (!isEnabled(sectionRule)) continue;
    await checkSectionReferences(
      markdownFiles,
      documentRoot,
      root,
      sectionRule,
      errors,
    );
  }

  return { errors, warnings };
}

export function formatDiagnostics(result: CheckResult): string[] {
  const lines: string[] = [];
  for (const diagnostic of result.warnings) {
    lines.push(`WARN: ${diagnostic.message}`);
  }
  for (const diagnostic of result.errors) {
    lines.push(`ERROR: ${diagnostic.message}`);
  }
  if (result.errors.length === 0 && result.warnings.length === 0) {
    lines.push("OK: documentation checks passed");
  } else if (result.errors.length === 0) {
    lines.push("OK: documentation checks passed with warnings");
  }
  return lines;
}

function checkLinks(
  files: MarkdownFile[],
  markdownPaths: Set<string>,
  extensions: string[],
  rule: LinkRuleConfig | undefined,
  errors: Diagnostic[],
): void {
  const externalPrefixes = rule?.externalPrefixes ?? DEFAULT_EXTERNAL_PREFIXES;
  const displayedPathPattern = compileRegExp(
    rule?.displayedMarkdownPathPattern,
    DEFAULT_DISPLAYED_MARKDOWN_PATH_PATTERN,
  );

  for (const file of files) {
    for (const link of markdownLinks(file.text)) {
      const target = link.target.trim().split(/\s+/, 1)[0];
      if (!target || isExternalTarget(target, externalPrefixes)) continue;

      const targetPath = target.split("#", 1)[0];
      if (!targetPath || !extensions.includes(extname(targetPath))) continue;

      const resolvedPath = normalizePath(
        resolve(dirname(file.path), targetPath),
      );
      if (!markdownPaths.has(resolvedPath)) {
        errors.push(
          error(`broken markdown link: ${file.relativePath} -> ${target}`),
        );
        continue;
      }

      if (rule?.checkDisplayedMarkdownFilename === false) continue;

      const targetBasename = basename(targetPath);
      const displayedTarget = stripBackticks(link.text.trim());
      if (displayedPathPattern.test(displayedTarget)) {
        const displayedBasename = basename(displayedTarget);
        if (displayedBasename !== targetBasename) {
          errors.push(
            error(
              `markdown link text does not match target filename: ${file.relativePath} has [${link.text}] -> ${targetBasename}`,
            ),
          );
        }
      }
    }
  }
}

function checkNumberedHeadings(
  files: MarkdownFile[],
  rule: NumberedDocumentConfig,
  errors: Diagnostic[],
): void {
  const filenamePattern = new RegExp(rule.filenamePattern);
  const headingPattern = new RegExp(rule.headingPattern);

  for (const file of files) {
    const filenameMatch = file.basename.match(filenamePattern);
    if (!filenameMatch) continue;

    const filenameNumber = extractGroup(filenameMatch, "number");
    if (!filenameNumber) continue;

    const firstHeading = file.text
      .split("\n")
      .find((line) => line.startsWith("# "));
    if (!firstHeading) {
      errors.push(error(`missing H1 heading: ${file.relativePath}`));
      continue;
    }

    const headingNumber = extractGroup(
      firstHeading.match(headingPattern),
      "number",
    );
    if (!headingNumber) {
      errors.push(
        error(
          `H1 heading does not match configured numbered-heading pattern: ${file.relativePath}`,
        ),
      );
    } else if (headingNumber !== filenameNumber) {
      errors.push(
        error(
          `filename/H1 number mismatch: ${file.relativePath} has H1 ${headingNumber}`,
        ),
      );
    }
  }
}

function checkDirectorySequences(
  files: MarkdownFile[],
  rule: DirectorySequenceConfig,
  config: CheckConfig,
  errors: Diagnostic[],
): void {
  const filenamePatternSource =
    rule.filenamePattern ?? config.numberedDocuments?.filenamePattern;
  if (!filenamePatternSource) {
    errors.push(
      error(
        "directory sequence rule requires filenamePattern or numberedDocuments.filenamePattern",
      ),
    );
    return;
  }

  const filenamePattern = new RegExp(filenamePatternSource);
  const includeDirs = compilePatterns(rule.includeDirs);
  const excludeDirs = compilePatterns(rule.excludeDirs);
  const directories = new Map<string, DirectorySequenceEntry[]>();

  for (const file of files) {
    const match = file.basename.match(filenamePattern);
    const numberText = extractGroup(match, "number");
    if (!numberText) continue;

    const dir = toSlash(dirname(file.relativePath));
    if (!isIncludedDirectory(dir, includeDirs, excludeDirs)) continue;

    const number = Number.parseInt(numberText, 10);
    const entries = directories.get(dir) ?? [];
    entries.push({ number, numberText });
    directories.set(dir, entries);
  }

  const startAt = rule.startAt ?? 0;
  const minFiles = rule.minFiles ?? 1;
  for (const [dir, entries] of directories) {
    if (entries.length < minFiles) continue;

    entries.sort((left, right) => left.number - right.number);
    const actualNumbers = entries.map((entry) => entry.number);
    const expectedNumbers = entries.map((_, index) => index + startAt);
    if (sameNumberList(actualNumbers, expectedNumbers)) continue;

    const width = Math.max(
      2,
      ...entries.map((entry) => entry.numberText.length),
    );
    errors.push(
      error(
        `numbered markdown files are not continuous in ${dir}: expected ${expectedNumbers
          .map((number) => formatSequenceNumber(number, width))
          .join(", ")}, got ${actualNumbers
          .map((number) => formatSequenceNumber(number, width))
          .join(", ")}`,
      ),
    );
  }
}

function compilePatterns(patterns: string[] | undefined): RegExp[] {
  return patterns?.map((pattern) => new RegExp(pattern)) ?? [];
}

function isIncludedDirectory(
  dir: string,
  includeDirs: RegExp[],
  excludeDirs: RegExp[],
): boolean {
  return (
    (includeDirs.length === 0 ||
      includeDirs.some((pattern) => pattern.test(dir))) &&
    !excludeDirs.some((pattern) => pattern.test(dir))
  );
}

function formatSequenceNumber(value: number, width: number): string {
  return value.toString().padStart(width, "0");
}

function checkBareNumberedReferences(
  files: MarkdownFile[],
  markdownBasenames: Set<string>,
  rule: NumberedDocumentConfig,
  errors: Diagnostic[],
): void {
  if (!rule.referencePattern) return;

  const referencePattern = ensureGlobalRegExp(rule.referencePattern);
  for (const file of files) {
    for (const match of file.text.matchAll(referencePattern)) {
      const filename = extractGroup(match, "filename");
      if (!filename || markdownBasenames.has(filename)) continue;
      errors.push(
        error(
          `unknown numbered markdown filename reference in ${file.relativePath}: ${filename}`,
        ),
      );
    }
  }
}

async function checkSectionReferences(
  files: MarkdownFile[],
  documentRoot: string,
  projectRoot: string,
  rule: SectionReferenceConfig,
  errors: Diagnostic[],
): Promise<void> {
  const targetFile = resolveAgainst(documentRoot, rule.targetFile);
  const sections = await readSections(targetFile, projectRoot, rule, errors);
  const refPattern = ensureGlobalRegExp(rule.sectionRefPattern);

  for (const file of files) {
    const lines = file.text.split("\n");
    lines.forEach((line, index) => {
      if (!lineMatchesIncludes(line, rule.lineIncludes)) return;
      for (const match of line.matchAll(refPattern)) {
        const section = extractGroup(match, "section");
        if (!section || sections.has(section)) continue;
        errors.push(
          error(
            `stale ${rule.label} section reference in ${file.relativePath}:${
              index + 1
            }: §${section}`,
          ),
        );
      }
    });
  }
}

async function readMarkdownFiles(
  dir: string,
  root: string,
  extensions: string[],
): Promise<MarkdownFile[]> {
  const files: MarkdownFile[] = [];
  for await (const entry of walk(dir, {
    exts: extensions,
    includeDirs: false,
    includeFiles: true,
  })) {
    files.push({
      path: normalizePath(entry.path),
      relativePath: relativePath(root, entry.path),
      basename: basename(entry.path),
      text: await deno.readTextFile(entry.path),
    });
  }
  files.sort((a, b) => a.relativePath.localeCompare(b.relativePath));
  return files;
}

async function readSections(
  targetFile: string,
  projectRoot: string,
  rule: SectionReferenceConfig,
  errors: Diagnostic[],
): Promise<Set<string>> {
  if (!(await isFile(targetFile))) {
    errors.push(
      error(
        `missing ${rule.label} section target: ${relativePath(
          projectRoot,
          targetFile,
        )}`,
      ),
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

function normalizeConfig(
  raw: Partial<CheckConfig>,
  source: string,
): CheckConfig {
  if (typeof raw.documents?.root !== "string" || !raw.documents.root) {
    throw new Error(
      `invalid docs checker config in ${source}: documents.root is required`,
    );
  }

  return {
    projectRoot: raw.projectRoot,
    documents: {
      root: raw.documents.root,
      extensions: raw.documents.extensions ?? DEFAULT_DOCUMENT_EXTENSIONS,
    },
    links: raw.links,
    numberedDocuments: raw.numberedDocuments,
    directorySequences: raw.directorySequences ?? [],
    sectionReferences: raw.sectionReferences ?? [],
  };
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

function isEnabled<T extends { enabled?: boolean }>(
  rule: T | undefined,
): rule is T {
  return rule !== undefined && rule.enabled !== false;
}

function isExternalTarget(target: string, prefixes: string[]): boolean {
  return prefixes.some((prefix) => target.startsWith(prefix));
}

async function isDirectory(path: string): Promise<boolean> {
  try {
    return (await deno.stat(path)).isDirectory;
  } catch (error) {
    if (error instanceof deno.errors.NotFound) return false;
    throw error;
  }
}

async function isFile(path: string): Promise<boolean> {
  try {
    return (await deno.stat(path)).isFile;
  } catch (error) {
    if (error instanceof deno.errors.NotFound) return false;
    throw error;
  }
}

function resolveAgainst(base: string, target: string): string {
  return normalizePath(isAbsolute(target) ? target : resolve(base, target));
}

function relativePath(root: string, path: string): string {
  return toSlash(relative(root, path)) || ".";
}

function normalizePath(path: string): string {
  return toSlash(normalize(path));
}

function toSlash(path: string): string {
  return path.replaceAll("\\", "/");
}

function stripBackticks(text: string): string {
  return text.startsWith("`") && text.endsWith("`") ? text.slice(1, -1) : text;
}

function sameNumberList(left: number[], right: number[]): boolean {
  return (
    left.length === right.length &&
    left.every((value, index) => value === right[index])
  );
}

function error(message: string): Diagnostic {
  return { severity: "error", message };
}
