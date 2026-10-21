import { MAX_CSP_REPORT_BYTES, parseCspReport } from "@/lib/csp-report";

const FORWARD_TIMEOUT_MS = 5_000;

function coordinatorBaseUrl(): string {
  const base =
    process.env.COORDINATOR_URL?.trim() ||
    process.env.NEXT_PUBLIC_COORDINATOR_URL?.trim() ||
    "http://localhost:8080";
  return base.replace(/\/+$/, "");
}

async function forwardReport(raw: string): Promise<void> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), FORWARD_TIMEOUT_MS);
  try {
    const response = await fetch(`${coordinatorBaseUrl()}/api/csp/report`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: raw,
      signal: controller.signal,
    });
    if (!response.ok) {
      console.warn(`[csp-report] coordinator responded ${response.status}`);
    }
  } finally {
    clearTimeout(timer);
  }
}

export async function POST(request: Request): Promise<Response> {
  try {
    const buffer = await request.arrayBuffer();
    if (buffer.byteLength > 0 && buffer.byteLength <= MAX_CSP_REPORT_BYTES) {
      const raw = new TextDecoder().decode(buffer);
      if (parseCspReport(raw)) {
        await forwardReport(raw);
      }
    }
  } catch (error) {
    console.warn("[csp-report] dropped report:", error);
  }
  return new Response(null, { status: 204 });
}
