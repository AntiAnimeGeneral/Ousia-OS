import { parseArgs } from "@std/cli/parse-args";
import { dirname, resolve } from "@std/path";
import { checkDocs, formatDiagnostics, loadConfig } from "./check-docs-lib.ts";
import { deno } from "./deno-runtime.ts";

const cliArgs = deno.args[0] === "--" ? deno.args.slice(1) : deno.args;
const args = parseArgs(cliArgs, {
  string: ["config", "root"],
  alias: { c: "config", r: "root" },
});
const configPath = resolve(
  deno.cwd(),
  optionalString(args.config) ?? "check-docs.config.json",
);
const config = await loadConfig(configPath);
const root = resolveRoot(
  configPath,
  config.projectRoot,
  optionalString(args.root) ?? optionalString(args._[0]),
);
const result = await checkDocs(root, config);

for (const line of formatDiagnostics(result)) {
  if (line.startsWith("ERROR:") || line.startsWith("WARN:")) {
    console.error(line);
  } else {
    console.log(line);
  }
}

if (result.errors.length > 0) {
  deno.exit(1);
}

function resolveRoot(
  configPath: string,
  configuredRoot: string | undefined,
  rootOverride: string | undefined,
): string {
  if (rootOverride) return resolve(deno.cwd(), rootOverride);
  return resolve(dirname(configPath), configuredRoot ?? ".");
}

function optionalString(value: unknown): string | undefined {
  if (typeof value === "string") return value;
  if (typeof value === "number") return value.toString();
  return undefined;
}
