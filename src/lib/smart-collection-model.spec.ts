import { describe, expect, it } from "vitest";
import {
  createDefaultSmartCollectionRule,
  resetSmartCollectionRuleOperator,
  smartCollectionOperators,
  smartCollectionRulesAreComplete,
} from "./smart-collection-model";

describe("smart collection model", () => {
  it("offers regex operators for text fields", () => {
    expect(smartCollectionOperators("artist")).toContain("matchesRegex");
  });

  it("offers comparison operators for numeric fields", () => {
    expect(smartCollectionOperators("year")).toContain("greaterThan");
    expect(smartCollectionOperators("rating")).toContain("greaterThanOrEqual");
  });

  it("resets an incompatible operator when a rule field changes", () => {
    const rule = { field: "year", operator: "matchesRegex", value: "2000" } as const;
    const editable = { ...rule };

    resetSmartCollectionRuleOperator(editable);

    expect(editable.operator).toBe("equals");
  });

  it("requires a non-empty value in every rule", () => {
    const rule = createDefaultSmartCollectionRule();

    expect(smartCollectionRulesAreComplete([rule])).toBe(false);
  });
});
