import { describe, expect, it } from "vitest";
import { formatBytes, formatEta, formatSpeed, percent } from "../../src/lib/format";

describe("formatBytes", () => {
  it("formats bytes without unit jump", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1024)).toBe("1.0 KB");
    expect(formatBytes(1024 * 1024 * 15)).toBe("15.0 MB");
    expect(formatBytes(1024 ** 3)).toBe("1.0 GB");
  });

  it("handles invalid input", () => {
    expect(formatBytes(-5)).toBe("—");
    expect(formatBytes(Number.NaN)).toBe("—");
  });
});

describe("formatSpeed / formatEta", () => {
  it("speed with unit", () => {
    expect(formatSpeed(1024 * 200)).toBe("200 KB/s");
    expect(formatSpeed(1024 * 5)).toBe("5.0 KB/s");
    expect(formatSpeed(0)).toBe("—");
  });

  it("eta rounding", () => {
    expect(formatEta(30 * 1024 * 1024, 1024 * 1024)).toBe("30 秒");
    expect(formatEta(90 * 1024 * 1024, 1024 * 1024)).toBe("1 分 30 秒");
    expect(formatEta(1024 * 1024, 0)).toBe("—");
    expect(formatEta(0, 1024)).toBe("0 秒");
  });
});

describe("percent", () => {
  it("clamps to 0..100", () => {
    expect(percent(50, 100)).toBe(50);
    expect(percent(10, 0)).toBe(0);
    expect(percent(200, 100)).toBe(100);
    expect(percent(-1, 100)).toBe(0);
  });
});
