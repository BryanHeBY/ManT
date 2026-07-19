/**
 * @file Contains the classic desktop-style menu bar, popup menu, and keyboard
 * help overlay. Menu actions remain in the app controller; this module only
 * renders their state and invokes supplied callbacks.
 */

export const MENU_BAR = [
  { id: "file", label: "File", left: 0 },
  { id: "view", label: "View", left: 6 },
  { id: "navigate", label: "Navigate", left: 12 },
  { id: "search", label: "Search", left: 22 },
  { id: "help", label: "Help", left: 30 },
] as const;

export type MenuId = (typeof MENU_BAR)[number]["id"];

export interface MenuEntry {
  label: string;
  shortcut?: string;
  checked?: boolean;
  action: () => void;
}

export interface MenuBarProps {
  topic: string;
  section: string | undefined;
  openMenu: MenuId | null;
  onToggleMenu: (menu: MenuId) => void;
}

/** Renders top-level menu labels and the current manual identifier. */
export function MenuBar({ topic, section, openMenu, onToggleMenu }: MenuBarProps) {
  return (
    <box
      height={1}
      flexDirection="row"
      backgroundColor="#181825"
      border={["bottom"]}
      borderColor="#313244"
    >
      {MENU_BAR.map((menu) => {
        const isOpen = openMenu === menu.id;
        return (
          <box
            key={menu.id}
            height={1}
            paddingLeft={1}
            paddingRight={1}
            backgroundColor={isOpen ? "#45475a" : "#181825"}
            onMouseDown={() => onToggleMenu(menu.id)}
          >
            <text fg={isOpen ? "#f5e0dc" : "#bac2de"}>{menu.label}</text>
          </box>
        );
      })}
      <box flexGrow={1} flexDirection="row" justifyContent="flex-end" paddingRight={1}>
        <text fg="#7f849c" truncate wrapMode="none">{`${topic}${section ? `(${section})` : ""}`}</text>
      </box>
    </box>
  );
}

export interface MenuPopupProps {
  menu: MenuId;
  entries: MenuEntry[];
  cursor: number;
  onActivate: (entry: MenuEntry) => void;
}

/** Renders the active popup directly below its top-level menu label. */
export function MenuPopup({ menu, entries, cursor, onActivate }: MenuPopupProps) {
  return (
    <box
      position="absolute"
      left={MENU_BAR.find((item) => item.id === menu)!.left}
      top={1}
      width={30}
      flexDirection="column"
      zIndex={10}
      backgroundColor="#1e1e2e"
      border={["left", "right", "bottom"]}
      borderColor="#585b70"
    >
      {entries.map((entry, index) => {
        const isActive = index === cursor;
        return (
          <box
            key={`${menu}-${entry.label}`}
            height={1}
            flexDirection="row"
            paddingLeft={1}
            paddingRight={1}
            backgroundColor={isActive ? "#45475a" : "#1e1e2e"}
            onMouseDown={(event) => {
              event.stopPropagation();
              onActivate(entry);
            }}
          >
            <text fg={isActive ? "#f5e0dc" : "#cdd6f4"}>
              {entry.checked ? "✓ " : "  "}
              {entry.label}
            </text>
            <box flexGrow={1} />
            <text fg={isActive ? "#bac2de" : "#7f849c"}>{entry.shortcut}</text>
          </box>
        );
      })}
    </box>
  );
}

/** Displays the discoverable keyboard command reference. */
export function KeyboardHelpDialog({ terminalWidth }: { terminalWidth: number }) {
  return (
    <box
      position="absolute"
      left={Math.max(2, Math.floor((terminalWidth - 54) / 2))}
      top={3}
      width={Math.min(54, terminalWidth - 4)}
      flexDirection="column"
      zIndex={20}
      backgroundColor="#1e1e2e"
      border={["top", "right", "bottom", "left"]}
      borderColor="#89b4fa"
      padding={1}
    >
      <text fg="#89b4fa"><b>Keyboard Shortcuts</b></text>
      <text fg="#cdd6f4">↑/↓ or j/k  select section</text>
      <text fg="#cdd6f4">←/→ or h/l  move through the section tree</text>
      <text fg="#cdd6f4">Enter        fold or unfold selected section</text>
      <text fg="#cdd6f4">Ctrl+F or /   find in current page</text>
      <text fg="#cdd6f4">n / N        next / previous search match</text>
      <text fg="#cdd6f4">F10          open menu bar</text>
      <text fg="#cdd6f4">q            quit</text>
      <box height={1} />
      <text fg="#7f849c">Esc or ? closes this window</text>
    </box>
  );
}
