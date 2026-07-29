import { describe, expect, it } from "vitest";
import {
  COLLECTION_DRAG_TYPE,
  readCollectionDragPayload,
  writeCollectionDragPayload,
} from "./collection-api";
import { createOnlineTrack } from "../test/fixtures";

class TestDataTransfer {
  readonly values = new Map<string, string>();
  effectAllowed = "none";
  dropEffect = "none";

  get types() {
    return [...this.values.keys()];
  }

  setData(type: string, value: string) {
    this.values.set(type, value);
  }

  getData(type: string) {
    return this.values.get(type) ?? "";
  }
}

describe("Collection drag payload", () => {
  it("round-trips a Local Music snapshot selection", () => {
    const dataTransfer = new TestDataTransfer();
    const payload = {
      kind: "local" as const,
      snapshotId: "snapshot-1",
      selection: {
        selectAll: false,
        ranges: [{ start: 2, end: 4 }],
        excludedRanges: [],
      },
    };

    writeCollectionDragPayload(dataTransfer as unknown as DataTransfer, payload);

    expect({
      effectAllowed: dataTransfer.effectAllowed,
      hasCustomType: dataTransfer.types.includes(COLLECTION_DRAG_TYPE),
      payload: readCollectionDragPayload(dataTransfer as unknown as DataTransfer),
    }).toEqual({
      effectAllowed: "copy",
      hasCustomType: true,
      payload,
    });
  });

  it("round-trips selected online tracks", () => {
    const dataTransfer = new TestDataTransfer();
    const tracks = [createOnlineTrack({ key: "one" }), createOnlineTrack({ key: "two" })];

    writeCollectionDragPayload(dataTransfer as unknown as DataTransfer, {
      kind: "online",
      tracks,
    });

    expect(readCollectionDragPayload(dataTransfer as unknown as DataTransfer)).toEqual({
      kind: "online",
      tracks,
    });
  });

  it("round-trips selected Collection items", () => {
    const dataTransfer = new TestDataTransfer();
    const payload = {
      kind: "collection" as const,
      sourceCollectionId: "collection-1",
      itemIds: ["item-1", "item-2"],
    };

    writeCollectionDragPayload(dataTransfer as unknown as DataTransfer, payload);

    expect(readCollectionDragPayload(dataTransfer as unknown as DataTransfer)).toEqual(payload);
  });

  it("rejects malformed external payloads", () => {
    const dataTransfer = new TestDataTransfer();
    dataTransfer.setData(COLLECTION_DRAG_TYPE, '{"kind":"online","tracks":[null]}');

    expect(readCollectionDragPayload(dataTransfer as unknown as DataTransfer)).toBeNull();
  });
});
