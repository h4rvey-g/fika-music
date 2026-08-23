import type {
  SmartCollectionField,
  SmartCollectionOperator,
  SmartCollectionRule,
} from "../generated/bindings";

export const SMART_COLLECTION_FIELD_OPTIONS: ReadonlyArray<{
  value: SmartCollectionField;
  label: string;
}> = [
  { value: "title", label: "Title" },
  { value: "artist", label: "Artist" },
  { value: "album", label: "Album" },
  { value: "albumArtist", label: "Album artist" },
  { value: "genre", label: "Genre" },
  { value: "year", label: "Year" },
  { value: "codec", label: "Codec" },
  { value: "bitrateKbps", label: "Bitrate" },
  { value: "sampleRateHz", label: "Sample rate" },
  { value: "durationSeconds", label: "Duration (seconds)" },
  { value: "trackNumber", label: "Track number" },
  { value: "discNumber", label: "Disc number" },
  { value: "fileName", label: "File name" },
  { value: "filePath", label: "File path" },
  { value: "fileSizeBytes", label: "File size (bytes)" },
  { value: "modifiedAt", label: "Modified timestamp" },
  { value: "indexedAt", label: "Indexed timestamp" },
  { value: "playCount", label: "Play count" },
  { value: "rating", label: "Rating" },
];

const NUMERIC_FIELDS = new Set<SmartCollectionField>([
  "year",
  "bitrateKbps",
  "sampleRateHz",
  "durationSeconds",
  "trackNumber",
  "discNumber",
  "fileSizeBytes",
  "modifiedAt",
  "indexedAt",
  "playCount",
  "rating",
]);

const TEXT_OPERATORS: ReadonlyArray<SmartCollectionOperator> = [
  "equals",
  "notEquals",
  "contains",
  "doesNotContain",
  "matchesRegex",
  "doesNotMatchRegex",
];

const NUMERIC_OPERATORS: ReadonlyArray<SmartCollectionOperator> = [
  "equals",
  "notEquals",
  "greaterThan",
  "greaterThanOrEqual",
  "lessThan",
  "lessThanOrEqual",
];

export const SMART_COLLECTION_OPERATOR_LABELS: Readonly<
  Record<SmartCollectionOperator, string>
> = {
  equals: "equals",
  notEquals: "does not equal",
  contains: "contains",
  doesNotContain: "does not contain",
  greaterThan: "is greater than",
  greaterThanOrEqual: "is at least",
  lessThan: "is less than",
  lessThanOrEqual: "is at most",
  matchesRegex: "matches regex",
  doesNotMatchRegex: "does not match regex",
};

export function isNumericSmartCollectionField(field: SmartCollectionField) {
  return NUMERIC_FIELDS.has(field);
}

export function smartCollectionOperators(field: SmartCollectionField) {
  return isNumericSmartCollectionField(field) ? NUMERIC_OPERATORS : TEXT_OPERATORS;
}

export function createDefaultSmartCollectionRule(): SmartCollectionRule {
  return { field: "artist", operator: "equals", value: "" };
}

export function resetSmartCollectionRuleOperator(rule: SmartCollectionRule) {
  const operators = smartCollectionOperators(rule.field);
  if (!operators.includes(rule.operator)) rule.operator = operators[0];
}

export function smartCollectionRulesAreComplete(rules: SmartCollectionRule[]) {
  return rules.length > 0 && rules.every((rule) => rule.value.trim().length > 0);
}
