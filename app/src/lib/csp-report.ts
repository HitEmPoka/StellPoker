export const MAX_CSP_REPORT_BYTES = 16 * 1024;

const MAX_FIELD_CHARS = 256;

export interface CspViolation {
  documentUri: string | null;
  violatedDirective: string | null;
  effectiveDirective: string | null;
  blockedUri: string | null;
  sourceFile: string | null;
  disposition: string;
  lineNumber: number | null;
  columnNumber: number | null;
  originalPolicy: string | null;
}

function asBoundedString(value: unknown): string | null {
  if (typeof value !== "string" || value.length === 0) return null;
  return value.length > MAX_FIELD_CHARS ? value.slice(0, MAX_FIELD_CHARS) : value;
}

function asLineNumber(value: unknown): number | null {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) {
    return null;
  }
  return Math.trunc(value);
}

function normalizeDisposition(value: unknown): string {
  if (typeof value === "string") {
    const disposition = value.toLowerCase();
    if (disposition === "enforce" || disposition === "report") {
      return disposition;
    }
  }
  return "unknown";
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function normalizeViolation(raw: Record<string, unknown>): CspViolation | null {
  const violatedDirective = asBoundedString(
    raw["violated-directive"] ?? raw.violated_directive,
  );
  const effectiveDirective = asBoundedString(
    raw["effective-directive"] ?? raw.effective_directive,
  );
  if (!violatedDirective && !effectiveDirective) return null;
  return {
    documentUri: asBoundedString(raw["document-uri"] ?? raw.document_uri),
    violatedDirective,
    effectiveDirective,
    blockedUri: asBoundedString(raw["blocked-uri"] ?? raw.blocked_uri),
    sourceFile: asBoundedString(raw["source-file"] ?? raw.source_file),
    disposition: normalizeDisposition(raw.disposition),
    lineNumber: asLineNumber(raw["line-number"] ?? raw.line_number),
    columnNumber: asLineNumber(raw["column-number"] ?? raw.column_number),
    originalPolicy: asBoundedString(
      raw["original-policy"] ?? raw.original_policy,
    ),
  };
}

export function parseCspReport(raw: string): CspViolation[] | null {
  const byteLength = new TextEncoder().encode(raw).byteLength;
  if (byteLength === 0 || byteLength > MAX_CSP_REPORT_BYTES) return null;

  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return null;
  }

  if (Array.isArray(value)) {
    const violations: CspViolation[] = [];
    for (const entry of value) {
      if (!isRecord(entry)) return null;
      if (entry.type !== undefined && entry.type !== "csp") continue;
      if (!isRecord(entry.body)) return null;
      const violation = normalizeViolation(entry.body);
      if (violation) violations.push(violation);
    }
    return violations.length > 0 ? violations : null;
  }

  if (!isRecord(value)) return null;
  const legacy = value["csp-report"];
  if (!isRecord(legacy)) return null;
  const violation = normalizeViolation(legacy);
  return violation ? [violation] : null;
}
