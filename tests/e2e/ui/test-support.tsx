/**
 * @file Shares rendering, frame-inspection, and OpenTUI timing helpers across
 * UI end-to-end tests.
 *
 * These tests deliberately inspect terminal frames rather than component
 * internals, so helpers here express stable user-visible coordinates.
 */

import { testRender } from "@opentui/react/test-utils";
import type { QueryResult } from "../../../src/query";
import { App } from "../../../src/ui/app";

export const NAV_WIDTH = 32;

export async function renderApp(
  result: QueryResult,
  options: { width?: number; height?: number; onQuit?: () => void } = {},
) {
  const setup = await testRender(
    <App result={result} onQuit={options.onQuit ?? (() => {})} />,
    { width: options.width ?? 80, height: options.height ?? 24 },
  );
  await setup.renderOnce();
  return setup;
}

export function navLines(frame: string): string[] {
  return frame.split("\n").map((line) => line.slice(0, NAV_WIDTH));
}

export function navigationSpans(
  lines: ReturnType<Awaited<ReturnType<typeof testRender>>["captureSpans"]>["lines"],
) {
  // Exclude the top menu and bottom status/search rows, which also begin at
  // column zero but do not belong to the sidebar.
  return lines.slice(1, -2).flatMap((line) => {
    let column = 0;
    return line.spans.filter((span) => {
      const startsInNavigation = column < NAV_WIDTH;
      column += span.text.length;
      return startsInNavigation;
    });
  });
}

export function navPosition(frame: string, label: string): { x: number; y: number } {
  const lines = navLines(frame);
  const y = lines.findIndex((line) => line.includes(label));
  if (y < 0) throw new Error(`Navigation item not found: ${label}`);
  return { x: lines[y]!.indexOf(label), y };
}

export function contentPosition(frame: string, label: string): { x: number; y: number } {
  const lines = frame.split("\n");
  for (let y = 0; y < lines.length; y++) {
    const x = lines[y]!.indexOf(label);
    if (x >= NAV_WIDTH) return { x, y };
  }
  throw new Error(`Content heading not found: ${label}`);
}

export async function flushKeyboard(setup: { flush: () => Promise<void> }): Promise<void> {
  // Keyboard events are delivered outside React's synthetic event queue. Give
  // React one turn to schedule the state update before capturing the frame.
  await new Promise<void>((resolve) => setTimeout(resolve, 10));
  await setup.flush();
}

export async function flushEscape(setup: { flush: () => Promise<void> }): Promise<void> {
  // Escape is an ANSI sequence, so the terminal parser waits briefly to
  // distinguish it from the start of a longer key sequence.
  await new Promise<void>((resolve) => setTimeout(resolve, 220));
  await setup.flush();
}

let didInstallWarningFilter = false;

/** Filters React act() warnings caused by OpenTUI's test renderer itself. */
export function installOpenTuiWarningFilter(): void {
  if (didInstallWarningFilter) return;
  didInstallWarningFilter = true;

  const originalConsoleError = console.error;
  console.error = (...args: unknown[]) => {
    const message = typeof args[0] === "string" ? args[0] : "";
    if (
      message.includes("was not wrapped in act")
      || message.includes("wrap tests with act")
    ) {
      return;
    }
    originalConsoleError.apply(console, args);
  };
}
