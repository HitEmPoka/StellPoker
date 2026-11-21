import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import "@testing-library/jest-dom";
import { useEffect, useState } from "react";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { Skeleton } from "@/components/Skeleton";
import { ErrorFallback } from "@/components/ErrorFallback";

const originalFetch = globalThis.fetch;

beforeEach(() => {
  vi.restoreAllMocks();
});

afterEach(() => {
  globalThis.fetch = originalFetch;
});

describe("loading state", () => {
  it("renders the Skeleton instead of a blank screen", () => {
    const { container } = render(<Skeleton />);
    expect(container.firstChild).toBeTruthy();
  });
});

describe("error UI", () => {
  it("renders ErrorFallback with message and a retry button", () => {
    const reset = vi.fn();
    render(<ErrorFallback error={new Error("coordinator timed out")} reset={reset} />);
    expect(screen.getByText(/TABLE ERROR/)).toBeInTheDocument();
    expect(screen.getByText(/coordinator timed out/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /RETRY/ })).toBeInTheDocument();
  });

  it("calls reset on retry", () => {
    const reset = vi.fn();
    render(<ErrorFallback error={new Error("boom")} reset={reset} />);
    fireEvent.click(screen.getByRole("button", { name: /RETRY/ }));
    expect(reset).toHaveBeenCalledTimes(1);
  });
});

describe("rejected fetch renders error UI, not a blank screen", () => {
  it("transitions loading -> error -> retry", async () => {
    globalThis.fetch = vi.fn(() => Promise.reject(new Error("network down")));

    function FetchWithRetry() {
      const [state, setState] = useState<"loading" | "error" | "success">("loading");
      useEffect(() => {
        void fetch("/api/table")
          .then(() => setState("success"))
          .catch(() => setState("error"));
      }, []);
      if (state === "loading") return <Skeleton />;
      if (state === "error")
        return (
          <ErrorFallback error={new Error("network down")} reset={() => setState("loading")} />
        );
      return <div data-testid="recovered">recovered</div>;
    }

    render(<FetchWithRetry />);

    // loading first: Skeleton shown, no error yet
    expect(screen.queryByText(/TABLE ERROR/)).not.toBeInTheDocument();
    expect(document.querySelector(".animate-pulse")).toBeTruthy();

    // fetch rejects -> error UI replaces the skeleton
    await waitFor(() => expect(screen.getByText(/TABLE ERROR/)).toBeInTheDocument());
    expect(screen.getByRole("button", { name: /RETRY/ })).toBeInTheDocument();

    // retry recovers back to loading state
    fireEvent.click(screen.getByRole("button", { name: /RETRY/ }));
    expect(document.querySelector(".animate-pulse")).toBeTruthy();
  });
});
