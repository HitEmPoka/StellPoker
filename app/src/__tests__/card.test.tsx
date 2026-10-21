import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { Card } from "@/components/Card";

// Card values encode suit * 13 + rank index (clubs, diamonds, hearts, spades; 2..A).
const ACE_OF_SPADES = 3 * 13 + 12;
const TEN_OF_HEARTS = 2 * 13 + 8;

describe("Card", () => {
  it("renders the card back when face down", () => {
    const { container } = render(<Card value={ACE_OF_SPADES} faceDown />);
    expect(container.textContent).toBe("S");
    expect(container.textContent).not.toContain("A");
  });

  it("renders the card back when no value is provided", () => {
    const { container } = render(<Card />);
    expect(container.textContent).toBe("S");
  });

  it("renders rank and suit for a face-up card", () => {
    const { container } = render(<Card value={ACE_OF_SPADES} />);
    expect(container.textContent).toContain("A");
    expect(container.textContent).toContain("♠");
    expect(container.querySelector(".animate-card-deal")).not.toBeNull();
  });

  it("names the card for screen readers", () => {
    const { getByRole } = render(<Card value={ACE_OF_SPADES} />);
    expect(getByRole("img", { name: "Ace of spades" })).toBeTruthy();
  });

  it("names a face down card without revealing it", () => {
    const { getByRole } = render(<Card value={ACE_OF_SPADES} faceDown />);
    expect(getByRole("img", { name: "Face down card" })).toBeTruthy();
  });

  it("renders red suits for hearts", () => {
    const { container } = render(<Card value={TEN_OF_HEARTS} />);
    expect(container.textContent).toContain("10");
    expect(container.textContent).toContain("♥");
  });

  it("applies size dimensions", () => {
    const { container } = render(<Card value={ACE_OF_SPADES} size="lg" />);
    const face = container.querySelector(".pixel-border-white") as HTMLElement;
    expect(face.style.width).toBe("72px");
    expect(face.style.height).toBe("100px");
  });

  it("renders both faces with flip animation variables when flipping", () => {
    const { container } = render(
      <Card value={ACE_OF_SPADES} flip flipDelay={0.2} dealFrom={{ x: 10, y: -40 }} />,
    );
    const flip = container.querySelector(".card-flip") as HTMLElement;
    expect(flip).not.toBeNull();
    expect(flip.style.getPropertyValue("--flip-delay")).toBe("0.2s");
    expect(flip.style.getPropertyValue("--deal-x")).toBe("10px");
    expect(flip.style.getPropertyValue("--deal-y")).toBe("-40px");
    expect(container.querySelector(".card-flip-back")).not.toBeNull();
    expect(container.querySelector(".card-flip-front")?.textContent).toContain("♠");
  });

  it("shows a hand strength label when strength is provided", () => {
    const { container } = render(<Card value={ACE_OF_SPADES} strength="strong" />);
    expect(container.textContent).toContain("STRONG");
  });
});
