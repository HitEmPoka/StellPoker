import { render, fireEvent } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { TokenSelector, type TokenChoice } from "../components/TokenSelector";

describe("TokenSelector", () => {
  it("defaults to XLM and allows switching to SAC", () => {
    let value: TokenChoice = { type: "XLM" };
    const onChange = (v: TokenChoice) => { value = v; };
    const { getByRole, getByPlaceholderText } = render(<TokenSelector value={value} onChange={onChange} />);

    const select = getByRole("combobox") as HTMLSelectElement;
    expect(select.value).toBe("XLM");

    fireEvent.change(select, { target: { value: "SAC" } });
    expect(value.type).toBe("SAC");

    const input = getByPlaceholderText("Enter SAC contract address") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "GABC..." } });
    expect(value.sacAddress).toBe("GABC...");
  });
});
