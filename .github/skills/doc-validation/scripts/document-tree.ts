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
import type { CheckConfig } from "./config.ts";
import { DEFAULT_DOCUMENT_EXTENSIONS } from "./config.ts";
import { DiagnosticBag } from "./diagnostics.ts";
import { deno } from "./deno-runtime.ts";

export interface MarkdownFile {
  path: string;
  relativePath: string;
  directory: string;
  basename: string;
  extension: string;
  text: string;
}

export interface DocumentTree {
  projectRoot: string;
  documentRoot: string;
  documentLabel: string;
  extensions: string[];
  files: MarkdownFile[];
  filePaths: Set<string>;
  fileBasenames: Set<string>;
}

export async function readDocumentTree(
  projectRoot: string,
  config: CheckConfig,
  diagnostics: DiagnosticBag,
): Promise<DocumentTree | undefined> {
  const root = normalizePath(await deno.realPath(projectRoot));
  const documentRoot = resolveAgainst(root, config.documents.root);
  const documentLabel = toSlash(config.documents.root);
  const extensions = config.documents.extensions ?? DEFAULT_DOCUMENT_EXTENSIONS;

  if (!(await isDirectory(documentRoot))) {
    diagnostics.error(`document root not found: ${documentLabel}`);
    return undefined;
  }

  const files = await readMarkdownFiles(documentRoot, root, extensions);
  return {
    projectRoot: root,
    documentRoot,
    documentLabel,
    extensions,
    files,
    filePaths: new Set(files.map((file) => file.path)),
    fileBasenames: new Set(files.map((file) => file.basename)),
  };
}

export async function isFile(path: string): Promise<boolean> {
  try {
    return (await deno.stat(path)).isFile;
  } catch (error) {
    if (error instanceof deno.errors.NotFound) return false;
    throw error;
  }
}

export function resolveAgainst(base: string, target: string): string {
  return normalizePath(isAbsolute(target) ? target : resolve(base, target));
}

export function relativePath(root: string, path: string): string {
  return toSlash(relative(root, path)) || ".";
}

export function normalizePath(path: string): string {
  return toSlash(normalize(path));
}

export function toSlash(path: string): string {
  return path.replaceAll("\\", "/");
}

export { basename, dirname, extname, resolve };

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
    const relativeFilePath = relativePath(root, entry.path);
    files.push({
      path: normalizePath(entry.path),
      relativePath: relativeFilePath,
      directory: toSlash(dirname(relativeFilePath)),
      basename: basename(entry.path),
      extension: extname(entry.path),
      text: await deno.readTextFile(entry.path),
    });
  }
  files.sort((left, right) =>
    left.relativePath.localeCompare(right.relativePath),
  );
  return files;
}

async function isDirectory(path: string): Promise<boolean> {
  try {
    return (await deno.stat(path)).isDirectory;
  } catch (error) {
    if (error instanceof deno.errors.NotFound) return false;
    throw error;
  }
}
