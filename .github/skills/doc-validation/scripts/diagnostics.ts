export type Severity = "error" | "warning";

export interface Diagnostic {
  severity: Severity;
  message: string;
}

export interface CheckResult {
  errors: Diagnostic[];
  warnings: Diagnostic[];
}

export class DiagnosticBag {
  readonly errors: Diagnostic[] = [];
  readonly warnings: Diagnostic[] = [];

  error(message: string): void {
    this.errors.push({ severity: "error", message });
  }

  warning(message: string): void {
    this.warnings.push({ severity: "warning", message });
  }

  toResult(): CheckResult {
    return {
      errors: [...this.errors],
      warnings: [...this.warnings],
    };
  }
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
