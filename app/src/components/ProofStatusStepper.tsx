"use client";

export type ProofStep = "shuffle" | "prove" | "verify" | "settle";

const STEPS: Array<{ key: ProofStep; label: string }> = [
  { key: "shuffle", label: "SHUFFLE" },
  { key: "prove", label: "PROVE" },
  { key: "verify", label: "VERIFY" },
  { key: "settle", label: "SETTLE" },
];

export function ProofStatusStepper({ active = "shuffle" }: { active?: ProofStep }) {
  const activeIndex = STEPS.findIndex((step) => step.key === active);
  return (
    <ol aria-label="Proof status" className="flex items-center justify-between gap-1" data-testid="proof-status-stepper">
      {STEPS.map((step, index) => {
        const complete = index < activeIndex;
        const current = index === activeIndex;
        return (
          <li key={step.key} className="flex flex-1 items-center gap-1">
            <span
              aria-current={current ? "step" : undefined}
              className="rounded px-2 py-1 text-[8px] font-bold"
              style={{
                color: complete || current ? "#f5e6c8" : "#7f8c8d",
                background: complete ? "#27ae60" : current ? "#c47d2e" : "rgba(127,140,141,.2)",
              }}
            >
              {complete ? "✓ " : ""}{step.label}
            </span>
            {index < STEPS.length - 1 && <span aria-hidden="true" className="flex-1 text-center text-[#7f8c8d]">›</span>}
          </li>
        );
      })}
    </ol>
  );
}
