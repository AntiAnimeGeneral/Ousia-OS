import { deno } from "./deno-runtime.ts";

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

export const DEFAULT_DOCUMENT_EXTENSIONS = [".md"];
export const DEFAULT_EXTERNAL_PREFIXES = [
  "http://",
  "https://",
  "mailto:",
  "#",
];
export const DEFAULT_DISPLAYED_MARKDOWN_PATH_PATTERN =
  /^(?:\.\.?\/)?(?:[^/\]]+\/)*[^/\]]+\.md$/;

export async function loadConfig(configPath: string): Promise<CheckConfig> {
  const raw = JSON.parse(
    await deno.readTextFile(configPath),
  ) as Partial<CheckConfig>;
  return normalizeConfig(raw, configPath);
}

export function normalizeConfig(
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

export function isEnabled<T extends { enabled?: boolean }>(
  rule: T | undefined,
): rule is T {
  return rule !== undefined && rule.enabled !== false;
}
