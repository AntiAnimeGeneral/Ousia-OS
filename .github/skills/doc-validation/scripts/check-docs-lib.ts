import { type CheckConfig, loadConfig, normalizeConfig } from "./config.ts";
import {
  type CheckResult,
  DiagnosticBag,
  formatDiagnostics,
} from "./diagnostics.ts";
import { readDocumentTree } from "./document-tree.ts";
import { runConfiguredRules } from "./rules.ts";

export type { CheckConfig } from "./config.ts";
export type { CheckResult, Diagnostic, Severity } from "./diagnostics.ts";
export { formatDiagnostics, loadConfig };

export async function checkDocs(
  projectRoot: string,
  config: CheckConfig,
): Promise<CheckResult> {
  const normalizedConfig = normalizeConfig(config, "inline config");
  const diagnostics = new DiagnosticBag();
  const tree = await readDocumentTree(
    projectRoot,
    normalizedConfig,
    diagnostics,
  );

  if (!tree) return diagnostics.toResult();

  await runConfiguredRules(normalizedConfig, tree, diagnostics);
  return diagnostics.toResult();
}
