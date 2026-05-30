declare module "@std/cli/parse-args" {
  export interface ParseArgsOptions {
    string?: string[];
    boolean?: string[];
    alias?: Record<string, string | string[]>;
    default?: Record<string, unknown>;
  }

  export type ParsedArgs =
    & { _: Array<string | number> }
    & Record<string, unknown>;

  export function parseArgs(
    args: string[],
    options?: ParseArgsOptions,
  ): ParsedArgs;
}

declare module "@std/fs/walk" {
  export interface WalkEntry {
    path: string;
    isFile: boolean;
    isDirectory: boolean;
    isSymlink: boolean;
  }

  export interface WalkOptions {
    maxDepth?: number;
    includeFiles?: boolean;
    includeDirs?: boolean;
    exts?: string[];
    skip?: RegExp[];
    followSymlinks?: boolean;
  }

  export function walk(
    root: string,
    options?: WalkOptions,
  ): AsyncIterableIterator<WalkEntry>;
}

declare module "@std/path" {
  export function basename(path: string, suffix?: string): string;
  export function dirname(path: string): string;
  export function extname(path: string): string;
  export function fromFileUrl(url: string | URL): string;
  export function isAbsolute(path: string): boolean;
  export function join(...paths: string[]): string;
  export function normalize(path: string): string;
  export function relative(from: string, to: string): string;
  export function resolve(...paths: string[]): string;
}
