import { describe, expect, test } from "vitest";
import { formatInterval, formatSpan } from "./format";

const minutes = (amount: number) => ({ unit: "minutes", amount }) as const;
const days = (amount: number) => ({ unit: "days", amount }) as const;

describe("formatInterval", () => {
  test("learning steps in minutes, then hours", () => {
    expect(formatInterval(minutes(0))).toBe("<1m");
    expect(formatInterval(minutes(1))).toBe("1m");
    expect(formatInterval(minutes(10))).toBe("10m");
    expect(formatInterval(minutes(59))).toBe("59m");
    expect(formatInterval(minutes(60))).toBe("1h");
    expect(formatInterval(minutes(300))).toBe("5h");
  });

  test("a step of a day or more reads as days", () => {
    expect(formatInterval(minutes(1440))).toBe("1d");
    expect(formatInterval(minutes(4320))).toBe("3d");
  });

  test("days, months and years", () => {
    expect(formatInterval(days(0))).toBe("<1d");
    expect(formatInterval(days(1))).toBe("1d");
    expect(formatInterval(days(29))).toBe("29d");
    expect(formatInterval(days(30))).toBe("1mo");
    expect(formatInterval(days(76))).toBe("2.5mo");
    expect(formatInterval(days(364))).toBe("12mo");
    expect(formatInterval(days(365))).toBe("1y");
    expect(formatInterval(days(450))).toBe("1.2y");
    expect(formatInterval(days(3650))).toBe("10y");
  });
});

describe("formatSpan", () => {
  test("seconds, minutes, hours", () => {
    expect(formatSpan(0)).toBe("0 seconds");
    expect(formatSpan(1000)).toBe("1 second");
    expect(formatSpan(40_000)).toBe("40 seconds");
    expect(formatSpan(60_000)).toBe("1 minute");
    expect(formatSpan(5 * 60_000 + 20_000)).toBe("5 minutes");
    expect(formatSpan(3_600_000)).toBe("1 hour");
    expect(formatSpan(3_900_000)).toBe("1 hour 5 minutes");
    expect(formatSpan(-5)).toBe("0 seconds");
  });
});
