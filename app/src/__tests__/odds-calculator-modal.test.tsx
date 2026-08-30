import { render, fireEvent, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { OddsCalculatorModal } from "../components/OddsCalculatorModal";

describe("OddsCalculatorModal (Issue #163)", () => {
  it("renders nothing when closed", () => {
    const { container } = render(
      <OddsCalculatorModal open={false} onClose={() => {}} />
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders the calculator UI when open", () => {
    render(<OddsCalculatorModal open onClose={() => {}} />);
    expect(screen.getByText("ODDS CALCULATOR")).toBeTruthy();
    expect(screen.getByText("YOUR HOLE CARDS")).toBeTruthy();
    expect(screen.getByText("BOARD (OPTIONAL)")).toBeTruthy();
    expect(screen.getByText("OPPONENTS")).toBeTruthy();
    expect(screen.getByText("CALCULATE ODDS")).toBeTruthy();
  });

  it("shows a validation error when calculating without both hole cards selected", () => {
    render(<OddsCalculatorModal open onClose={() => {}} />);
    fireEvent.click(screen.getByText("CALCULATE ODDS"));
    expect(screen.getByText("Select both hole cards")).toBeTruthy();
  });

  it("calculates and displays win/tie/loss once both hole cards are selected", () => {
    render(<OddsCalculatorModal open onClose={() => {}} />);

    fireEvent.change(screen.getByLabelText("Card 1 rank"), { target: { value: "12" } }); // Ace
    fireEvent.change(screen.getByLabelText("Card 1 suit"), { target: { value: "2" } }); // hearts
    fireEvent.change(screen.getByLabelText("Card 2 rank"), { target: { value: "12" } }); // Ace
    fireEvent.change(screen.getByLabelText("Card 2 suit"), { target: { value: "1" } }); // diamonds

    fireEvent.click(screen.getByText("CALCULATE ODDS"));

    expect(screen.getByText("WIN")).toBeTruthy();
    expect(screen.getByText("TIE")).toBeTruthy();
    expect(screen.getByText("LOSS")).toBeTruthy();
    expect(screen.getByText(/Estimated from [\d,]+ simulated hands\./)).toBeTruthy();
  });

  it("shows an error instead of crashing when the same card is picked twice", () => {
    render(<OddsCalculatorModal open onClose={() => {}} />);

    fireEvent.change(screen.getByLabelText("Card 1 rank"), { target: { value: "12" } });
    fireEvent.change(screen.getByLabelText("Card 1 suit"), { target: { value: "2" } });
    fireEvent.change(screen.getByLabelText("Card 2 rank"), { target: { value: "12" } });
    fireEvent.change(screen.getByLabelText("Card 2 suit"), { target: { value: "2" } }); // same card as Card 1

    fireEvent.click(screen.getByText("CALCULATE ODDS"));

    expect(screen.getByText("Duplicate cards are not allowed")).toBeTruthy();
  });

  it("calls onClose when the close button is clicked", () => {
    let closed = false;
    render(<OddsCalculatorModal open onClose={() => { closed = true; }} />);
    fireEvent.click(screen.getByText("✕"));
    expect(closed).toBe(true);
  });
});
