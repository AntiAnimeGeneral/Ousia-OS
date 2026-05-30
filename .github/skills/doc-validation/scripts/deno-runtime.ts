interface DenoFileInfo {
  isFile: boolean;
  isDirectory: boolean;
}

interface DenoRuntime {
  args: string[];
  errors: {
    NotFound: new (...args: unknown[]) => Error;
  };
  chdir(directory: string): void;
  cwd(): string;
  exit(code?: number): never;
  makeTempDir(): Promise<string>;
  mkdir(path: string, options?: { recursive?: boolean }): Promise<void>;
  readTextFile(path: string): Promise<string>;
  realPath(path: string): Promise<string>;
  remove(path: string, options?: { recursive?: boolean }): Promise<void>;
  stat(path: string): Promise<DenoFileInfo>;
  test(name: string, fn: () => void | Promise<void>): void;
  writeTextFile(path: string, data: string): Promise<void>;
}

export const deno = (globalThis as typeof globalThis & { Deno: DenoRuntime })
  .Deno;
