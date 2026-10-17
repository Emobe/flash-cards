import { expect, test } from "vitest";
import { dueRange } from "./DueSheet";

test("each choice gives its range of days", () => {
  expect(dueRange("today", "", "", "")).toEqual({ fromDays: 0, toDays: 0 });
  expect(dueRange("tomorrow", "", "", "")).toEqual({ fromDays: 1, toDays: 1 });
  expect(dueRange("in", " 12 ", "", "")).toEqual({ fromDays: 12, toDays: 12 });
  expect(dueRange("between", "", "0", "36500")).toEqual({ fromDays: 0, toDays: 36_500 });
  expect(dueRange("between", "", "4", "4")).toEqual({ fromDays: 4, toDays: 4 });
});

test("days that are not whole, too many or the wrong way round are refused", () => {
  const outside = { problem: "Enter whole days from 0 to 36,500." };
  expect(dueRange("in", "", "", "")).toEqual(outside);
  expect(dueRange("in", "2.5", "", "")).toEqual(outside);
  expect(dueRange("in", "-1", "", "")).toEqual(outside);
  expect(dueRange("in", "36501", "", "")).toEqual(outside);
  expect(dueRange("between", "", "3", "")).toEqual(outside);
  expect(dueRange("between", "", "8", "3")).toEqual({
    problem: "The first number of days cannot be more than the second.",
  });
});
