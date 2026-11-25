// The Question Desk frontend: renders the desk state from Rust and sends the user's intents.
// No framework and no build step. Each render builds fresh DOM and morphs it into the live
// tree, so focus, the caret, IME composition, selection and scroll positions survive.
"use strict";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const isMac = /Mac/.test(navigator.platform) || /Mac OS X/.test(navigator.userAgent);
document.documentElement.dataset.platform = isMac ? "macos" : /Win/.test(navigator.platform) ? "windows" : "linux";
const CMD = isMac ? "⌘" : "Ctrl+";
const SHIFT = isMac ? "⇧" : "Shift+";

// UI preferences (sidebar visibility and width); storage may be unavailable, which is fine.
const prefs = {
  get: (key) => { try { return localStorage.getItem(key); } catch { return null; } },
  set: (key, value) => { try { localStorage.setItem(key, value); } catch { /* not kept */ } },
};

let desk = null; // the last `state` from Rust
let sidebarOpen = prefs.get("sidebarOpen") !== "false";
let hovered = null; // { ticket, question, option } under the mouse, for the preview panel
let lastPage = ""; // ticket + step of the rendered page, to reset its scroll position
let lastKeyAction = { name: "", at: 0 }; // to drop the menu's echo of a handled shortcut

const el = {
  app: document.getElementById("app"),
  sidebar: document.getElementById("sidebar"),
  resizer: document.getElementById("resizer"),
  toolbar: document.getElementById("toolbar"),
  steps: document.getElementById("steps"),
  page: document.getElementById("page"),
  footer: document.getElementById("footer"),
  settings: document.getElementById("settings"),
};

const report = (level, message) => invoke("frontend_log", { level, message }).catch(() => {});
window.addEventListener("error", (e) => report("error", `${e.message} at ${e.filename}:${e.lineno}`));
window.addEventListener("unhandledrejection", (e) => report("error", `unhandled: ${e.reason}`));

// ---- Icons (Lucide, ISC; see assets/icons/) -------------------------------------------

const ICONS = {
  sidebar: '<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M9 3v18"/>',
  settings:
    '<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>',
  warn: '<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/>',
  done: '<path d="M21.801 10A10 10 0 1 1 17 3.335"/><path d="m9 11 3 3L22 4"/>',
  inbox:
    '<polyline points="22 12 16 12 14 15 10 15 8 12 2 12"/><path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/>',
  sparkles:
    '<path d="M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z"/><path d="M20 3v4"/><path d="M22 5h-4"/><path d="M4 17v2"/><path d="M5 18H3"/>',
  eye: '<path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0"/><circle cx="12" cy="12" r="3"/>',
};

function icon(name) {
  const t = document.createElement("template");
  t.innerHTML = `<svg class="icon" viewBox="0 0 24 24" aria-hidden="true">${ICONS[name]}</svg>`;
  return t.content.firstChild;
}

// ---- DOM building ---------------------------------------------------------------------

/** `h("div.a.b", { attrs }, ...children)`; `false`/`null` children are skipped. */
function h(spec, attrs, ...children) {
  const [tag, ...classes] = spec.split(".");
  const node = document.createElement(tag);
  if (classes.length) node.className = classes.join(" ");
  for (const [key, value] of Object.entries(attrs || {})) {
    if (value === false || value === null || value === undefined) continue;
    if (key === "html") node.innerHTML = value; // only sanitised Markdown from Rust
    else if (key === "checked" || key === "value") node[key] = value;
    else node.setAttribute(key, value === true ? "" : String(value));
  }
  for (const child of children.flat()) {
    if (child === false || child === null || child === undefined) continue;
    node.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return node;
}

/** Makes `live`'s children look like `next`, reusing nodes where tag and `data-key` match. */
function morphChildren(live, next) {
  const want = Array.isArray(next) ? next.filter(Boolean) : [next];
  const have = Array.from(live.childNodes);
  want.forEach((node, i) => {
    const old = have[i];
    if (!old) live.append(node);
    else if (same(old, node)) morph(old, node);
    else live.replaceChild(node, old);
  });
  for (const old of have.slice(want.length)) old.remove();
}

function same(a, b) {
  if (a.nodeType !== b.nodeType) return false;
  if (a.nodeType !== Node.ELEMENT_NODE) return true;
  return a.tagName === b.tagName && a.getAttribute("data-key") === b.getAttribute("data-key");
}

function morph(old, node) {
  if (old.nodeType !== Node.ELEMENT_NODE) {
    if (old.nodeValue !== node.nodeValue) old.nodeValue = node.nodeValue;
    return;
  }
  for (const { name } of Array.from(old.attributes)) {
    if (!node.hasAttribute(name)) old.removeAttribute(name);
  }
  for (const { name, value } of Array.from(node.attributes)) {
    if (old.getAttribute(name) !== value) old.setAttribute(name, value);
  }
  if (old.tagName === "INPUT") {
    old.checked = node.checked;
    if (old.value !== node.value && document.activeElement !== old) old.value = node.value;
  }
  if (old.tagName === "TEXTAREA") {
    // While the user types, the textarea is the truth; Rust's copy lags one round trip.
    if (document.activeElement !== old && old.value !== node.value) old.value = node.value;
    return;
  }
  // Markdown is replaced only when it changed, so a text selection in it survives.
  if (node.hasAttribute("data-html")) return;
  morphChildren(old, Array.from(node.childNodes));
}

// ---- State ----------------------------------------------------------------------------

let fetching = null;
let rendered = false;
let again = false;
async function refresh() {
  if (fetching) {
    again = true;
    return fetching;
  }
  fetching = (async () => {
    do {
      again = false;
      desk = await invoke("state");
      render();
      if (!rendered) {
        rendered = true;
        report("info", `first render: ${desk.openCount} open round(s), ${document.querySelectorAll("*").length} elements`);
      }
    } while (again);
  })();
  try {
    await fetching;
  } finally {
    fetching = null;
  }
}

/** Sends an intent, then re-renders. */
async function act(command, args) {
  await invoke(command, args);
  await refresh();
}

function current() {
  const round = desk && desk.round;
  if (!round || round.step === null) return null;
  return round.questions[round.step];
}

// ---- Rendering ------------------------------------------------------------------------

function render() {
  if (!desk) return;
  document.documentElement.dataset.platform = desk.platform;
  el.app.classList.toggle("sidebar-hidden", !sidebarOpen);
  morphChildren(el.sidebar, renderSidebar());
  morphChildren(el.toolbar, renderToolbar());
  morphChildren(el.steps, desk.round ? renderSteps(desk.round) : []);
  const round = desk.round;
  const pageKey = round ? `${round.ticket}:${round.step}` : "";
  morphChildren(el.page, h("div.column", { "data-key": pageKey }, renderPage()));
  morphChildren(el.footer, round ? renderFooter(round) : []);
  if (pageKey !== lastPage) {
    el.page.scrollTop = 0;
    lastPage = pageKey;
  }
}

function renderSidebar() {
  const rows = [];
  if (desk.sessions.length === 0) rows.push(h("div.nothing", {}, "Nothing waiting"));
  for (const session of desk.sessions) {
    rows.push(h("div.session-label", { title: session.label }, session.label));
    for (const r of session.rounds) {
      const meta = `${r.answered}/${r.total} answered · ${r.age}${r.connected ? "" : " · ⚠ disconnected"}`;
      rows.push(
        h(
          "button.round-row",
          {
            "data-key": r.ticket,
            "data-action": "select",
            "data-arg": r.ticket,
            "aria-current": r.selected ? "true" : "false",
          },
          h("div.row-title", {}, h("span", {}, r.name), r.new && h("span.new-badge", {}, "New")),
          h(r.connected ? "div.meta" : "div.meta.warn", {}, meta),
        ),
      );
    }
  }
  return [
    h("div.sidebar-top", { "data-tauri-drag-region": true }),
    h(
      "div.section-title",
      {},
      h("span", {}, "Open Rounds"),
      desk.openCount > 0 && h("span.count", {}, String(desk.openCount)),
    ),
    h("div.sidebar-scroll", {}, rows),
  ];
}

function renderToolbar() {
  const round = desk.round;
  const items = [
    h(
      "button.toolbar-button",
      {
        "data-action": "toggle-sidebar",
        title: sidebarOpen ? `Hide Sidebar (⌃${CMD}S)` : `Show Sidebar (⌃${CMD}S)`,
        "aria-label": sidebarOpen ? "Hide sidebar" : "Show sidebar",
      },
      icon("sidebar"),
    ),
  ];
  if (round) {
    items.push(
      h(
        "div.toolbar-title",
        { "data-tauri-drag-region": true },
        h("span.title", {}, round.title || "Question Desk"),
        h("span.subtitle", {}, round.session),
      ),
    );
    if (!round.connected) {
      items.push(
        h(
          "span.disconnected",
          {
            title:
              "No Agent call is waiting for this round. Your answers are kept; the Agent can pick them up with its ticket.",
          },
          icon("warn"),
          "Agent disconnected",
        ),
      );
    }
    if (round.step === null) {
      items.push(h("span.chip", {}, "Review"));
    } else {
      const q = round.questions[round.step];
      items.push(h("span.chip", { title: q.header }, q.header));
      items.push(h("span.position", {}, `${round.step + 1} / ${round.questions.length}`));
    }
  } else {
    items.push(
      h("div.toolbar-title", {}, h("span.title", {}, "Question Desk")),
    );
  }
  items.push(
    h(
      "button.toolbar-button",
      { "data-action": "settings", title: `Settings (${CMD},)`, "aria-label": "Settings" },
      icon("settings"),
    ),
  );
  return items;
}

function renderSteps(round) {
  const buttons = round.questions.map((q, i) =>
    h(
      `button.step.${q.state}`,
      {
        "data-action": "goto",
        "data-arg": i,
        title: q.question,
        "aria-label": `Question ${i + 1}: ${q.question} (${q.state})`,
        "aria-current": q.state === "current" ? "step" : false,
      },
      String(i + 1),
    ),
  );
  buttons.push(
    h(
      round.step === null ? "button.step.current" : "button.step",
      { "data-action": "review", "aria-current": round.step === null ? "step" : false },
      "Review",
    ),
  );
  return h("div.steps", { role: "navigation", "aria-label": "Questions" }, buttons);
}

function renderPage() {
  const round = desk.round;
  if (!round) {
    return h(
      "div.empty-state",
      {},
      icon("inbox"),
      h("h1", {}, "No open questions"),
      h("p.selectable", {}, desk.status),
    );
  }
  return round.step === null ? renderReview(round) : renderQuestion(round, round.step);
}

function renderQuestion(round, index) {
  const q = round.questions[index];
  const parts = [h("h1.question.selectable", {}, q.question)];
  if (q.bodyHtml) {
    parts.push(
      h(
        "div.body-card",
        {},
        h("div.markdown.selectable", { "data-key": q.bodyHtml, "data-html": true, html: q.bodyHtml }),
      ),
    );
  }
  if (q.recommendationText) {
    parts.push(
      h(
        "div.recommendation",
        {},
        icon("sparkles"),
        h("span.chip.accent", {}, "Recommended"),
        h("span.selectable", {}, q.recommendationText),
      ),
    );
  }
  parts.push(renderChoices(round, q, index));
  parts.push(renderNote(round, q, index));
  return parts;
}

function renderChoices(round, q, index) {
  if (q.options.length === 0) {
    return h("p.open-question", {}, "Open question: answer in the note below.");
  }
  const group = `q-${index}`;
  const cards = q.options.map((o, i) =>
    h(
      `label.option${o.selected ? ".selected" : ""}`,
      { "data-action": "toggle", "data-arg": i, "data-option": i },
      h("input", {
        type: q.multiSelect ? "checkbox" : "radio",
        name: group,
        checked: o.selected,
        "aria-describedby": o.description ? `${group}-d${i}` : false,
      }),
      h(
        "div.option-text",
        {},
        h(
          "div.option-title",
          {},
          i < 9 && h("kbd", { "aria-hidden": "true" }, String(i + 1)),
          h("span", {}, o.label),
          o.recommended && h("span.chip.accent", {}, "Recommended"),
        ),
        o.description && h("div.option-desc", { id: `${group}-d${i}` }, o.description),
      ),
    ),
  );
  const options = h(
    "div.options",
    { role: q.multiSelect ? "group" : "radiogroup", "aria-label": q.header },
    cards,
  );
  // The layout must not depend on what is hovered, only whether any Option has a preview.
  const hasPreviews = q.options.some((o) => o.preview);
  if (!hasPreviews) return h("div.choices", {}, options);
  const preview = previewFor(round, q, index);
  return h(
    "div.choices.has-previews",
    {},
    options,
    h(
      preview === null ? "aside.preview.empty" : "aside.preview",
      { "aria-label": "Preview" },
      h("div.preview-head", {}, icon("eye"), "PREVIEW"),
      preview === null
        ? h("p.hint", {}, "Hover an option to see its preview.")
        : h("pre.selectable", {}, preview),
    ),
  );
}

/** The preview of the hovered Option, else of the first selected one that has one. */
function previewFor(round, q, index) {
  if (hovered && hovered.ticket === round.ticket && hovered.question === index) {
    const o = q.options[hovered.option];
    if (o && o.preview) return o.preview;
  }
  const selected = q.options.find((o) => o.selected && o.preview);
  return selected ? selected.preview : null;
}

function renderNote(round, q, index) {
  const hint =
    q.options.length === 0
      ? "Your answer"
      : q.options.some((o) => o.selected)
        ? "Note (optional): caveats, reasons, “yes, but …”"
        : "Note: without a selected option, this text is your answer";
  return h(
    "div.note",
    {},
    h("label", { for: "note" }, "Note"),
    h("textarea", {
      id: "note",
      "data-key": `${round.ticket}:${index}`,
      rows: 4,
      placeholder: hint,
      spellcheck: "true",
      value: q.note,
    }),
  );
}

function renderReview(round) {
  const parts = [h("h1.review-title", {}, "Review your answers")];
  if (round.unanswered > 0) {
    parts.push(
      h("div.unanswered-line", {}, icon("warn"), `${round.unanswered} question(s) unanswered`),
    );
  }
  parts.push(
    h(
      "div.review-list",
      {},
      round.questions.map((q, i) => {
        const answered = q.answer !== null;
        return h(
          `button.review-item${answered ? "" : ".unanswered"}`,
          { "data-action": "goto", "data-arg": i, title: "Edit this answer" },
          h(
            "div.review-head",
            {},
            h(answered ? "span.ok" : "span.warn", {}, icon(answered ? "done" : "warn")),
            h("span.num", {}, `${i + 1}.`),
            h("span.chip", {}, q.header),
            h("span.q", {}, q.question),
          ),
          answered
            ? h("div.review-answer", {}, q.answer)
            : h("div.review-answer.missing", {}, "Unanswered"),
          q.answerNote && h("div.review-note", {}, `Note: ${q.answerNote}`),
        );
      }),
    ),
  );
  return parts;
}

function renderFooter(round) {
  const review = round.step === null;
  const hints = review
    ? [[`${CMD}↩`, "submit"], [`${CMD}${SHIFT}↩`, "back"], ["Esc", "cancel"]]
    : [
        ["1–9", "select"],
        ["Tab", "note"],
        ["↩", "accept recommendation"],
        [`${CMD}↩`, "next"],
        [`${CMD}${SHIFT}↩`, "back"],
        ["Esc", "leave note / cancel"],
      ];
  return [
    h(
      "div.footer-row",
      {},
      h("button", { "data-action": "cancel" }, "Cancel Round…"),
      h("span.spacer", {}),
      h("button.back", { "data-action": "back", disabled: round.step === 0 }, "Back"),
      h(
        "button.default",
        { "data-action": review ? "submit" : "next", "data-key": review ? "submit" : "next" },
        review ? "Submit" : "Next",
      ),
    ),
    h(
      "div.hints",
      { "aria-hidden": "true" },
      hints.map(([key, label]) => h("span", {}, h("kbd", {}, key), label)),
    ),
  ];
}

// ---- Intents --------------------------------------------------------------------------

const actions = {
  select: (ticket) => act("select", { ticket }),
  toggle: (i) => act("toggle_option", { index: Number(i) }),
  goto: (i) => act("go_to", { index: Number(i) }),
  review: () => act("review"),
  next: () => act("next"),
  back: () => act("back"),
  submit: () => act("request_submit"),
  cancel: () => act("request_cancel"),
  primary: () => (desk.round.step === null ? actions.submit() : actions.next()),
  settings: () => openSettings(),
  "toggle-sidebar": () => {
    sidebarOpen = !sidebarOpen;
    prefs.set("sidebarOpen", String(sidebarOpen));
    render();
  },
};

document.addEventListener("click", (e) => {
  const link = e.target.closest(".markdown a[href]");
  if (link) {
    e.preventDefault();
    invoke("open_link", { url: link.href }).catch(console.warn);
    return;
  }
  const target = e.target.closest("[data-action]");
  if (!target || target.disabled || !desk) return;
  // Option cards: the desk decides (radio choices can be cleared again), not the input.
  e.preventDefault();
  actions[target.dataset.action]?.(target.dataset.arg);
});

el.page.addEventListener("mouseover", (e) => {
  const card = e.target.closest("[data-option]");
  const round = desk && desk.round;
  if (!card || !round || round.step === null) return;
  const next = { ticket: round.ticket, question: round.step, option: Number(card.dataset.option) };
  if (!hovered || hovered.option !== next.option || hovered.question !== next.question || hovered.ticket !== next.ticket) {
    hovered = next;
    render();
  }
});
el.page.addEventListener("mouseout", (e) => {
  if (hovered && e.target.closest(".options") && !(e.relatedTarget && e.relatedTarget.closest?.(".options"))) {
    hovered = null;
    render();
  }
});

// The Note: every edit goes to the desk; IME pre-edit text only once it is committed.
el.page.addEventListener("input", (e) => {
  if (e.target.id !== "note" || e.isComposing) return;
  act("set_note", { note: e.target.value });
});
el.page.addEventListener("compositionend", (e) => {
  if (e.target.id === "note") act("set_note", { note: e.target.value });
});

// ---- Keyboard -------------------------------------------------------------------------

function isTextField(node) {
  return node && (node.tagName === "TEXTAREA" || (node.tagName === "INPUT" && /^(text|password|search|)$/.test(node.type)));
}

function keyAction(name, fn) {
  lastKeyAction = { name, at: performance.now() };
  fn();
}

document.addEventListener("keydown", (e) => {
  if (el.settings.open || !desk || !desk.round) return;
  if (e.isComposing || e.keyCode === 229) return;
  const round = desk.round;
  const mod = isMac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
  const plain = !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey;
  const active = document.activeElement;
  const inNote = active && active.id === "note";

  if (e.key === "Enter" && mod && !e.altKey) {
    e.preventDefault();
    if (e.shiftKey) keyAction("back", actions.back);
    else keyAction("next", actions.primary);
    return;
  }
  if (round.step === null) {
    if (e.key === "Escape" && plain) {
      e.preventDefault();
      actions.cancel();
    }
    return;
  }
  if (inNote) {
    if (e.key === "Escape" && plain) {
      e.preventDefault();
      active.blur();
    }
    return;
  }
  if (isTextField(active)) return;
  const q = round.questions[round.step];
  if (e.key === "Tab" && plain) {
    e.preventDefault();
    document.getElementById("note")?.focus();
  } else if (e.key === "Escape" && plain) {
    e.preventDefault();
    actions.cancel();
  } else if (e.key === "Enter" && plain && q.acceptsRecommendation && active?.tagName !== "BUTTON") {
    e.preventDefault();
    act("accept_recommendation");
  } else if (plain && /^[1-9]$/.test(e.key)) {
    const i = Number(e.key) - 1;
    if (i < q.options.length) {
      e.preventDefault();
      actions.toggle(i);
    }
  }
});

// ---- Menu -----------------------------------------------------------------------------

listen("menu", ({ payload }) => {
  // The menu's accelerator for a key the page just handled itself.
  if (lastKeyAction.name === payload && performance.now() - lastKeyAction.at < 250) return;
  if (payload === "settings") return el.settings.open || openSettings();
  if (payload === "toggle-sidebar") return actions["toggle-sidebar"]();
  if (el.settings.open || !desk || !desk.round) return;
  if (payload === "next") actions.primary();
  else if (actions[payload]) actions[payload]();
});

listen("desk-changed", () => refresh());

// Only text gets the native context menu (spelling, Look Up, Emoji & Symbols, copy).
document.addEventListener("contextmenu", (e) => {
  if (isTextField(e.target)) return;
  const selection = window.getSelection();
  if (e.target.closest(".selectable") && selection && !selection.isCollapsed) return;
  e.preventDefault();
});

// ---- Sidebar width --------------------------------------------------------------------

const savedWidth = Number(prefs.get("sidebarWidth"));
if (savedWidth >= 180 && savedWidth <= 480) {
  document.documentElement.style.setProperty("--sidebar-w", `${savedWidth}px`);
}
el.resizer.addEventListener("pointerdown", (e) => {
  e.preventDefault();
  el.resizer.setPointerCapture(e.pointerId);
  const move = (m) => {
    const width = Math.min(480, Math.max(180, m.clientX));
    document.documentElement.style.setProperty("--sidebar-w", `${width}px`);
    prefs.set("sidebarWidth", String(width));
  };
  const up = () => {
    el.resizer.removeEventListener("pointermove", move);
    el.resizer.removeEventListener("pointerup", up);
  };
  el.resizer.addEventListener("pointermove", move);
  el.resizer.addEventListener("pointerup", up);
});

// ---- Settings -------------------------------------------------------------------------

const form = document.getElementById("settings-form");
const field = (id) => document.getElementById(id);
let settingsError = null;

async function openSettings() {
  if (el.settings.open) return;
  const s = await invoke("settings");
  field("s-host").value = s.host;
  field("s-host").placeholder = s.defaultHost;
  field("s-port").value = s.port;
  field("s-port").placeholder = String(s.defaultPort);
  field("s-token").value = s.token;
  field("s-token").type = "password";
  field("s-show").checked = false;
  field("s-path").textContent = s.path ? `Saved in ${s.path}` : "";
  settingsError = null;
  await updateWarnings();
  el.settings.showModal();
  field("s-host").focus();
}

async function updateWarnings() {
  const warning = await invoke("settings_warning", {
    host: field("s-host").value,
    token: field("s-token").value,
  });
  const lines = [warning, settingsError].filter(Boolean);
  morphChildren(
    field("s-warnings"),
    lines.map((text) => h("div", {}, icon("warn"), h("span", {}, text))),
  );
}

field("s-host").addEventListener("input", updateWarnings);
field("s-token").addEventListener("input", updateWarnings);
field("s-show").addEventListener("change", (e) => {
  field("s-token").type = e.target.checked ? "text" : "password";
});
field("s-cancel").addEventListener("click", () => el.settings.close());
form.addEventListener("submit", async (e) => {
  e.preventDefault();
  try {
    await invoke("save_settings", {
      host: field("s-host").value,
      port: field("s-port").value,
      token: field("s-token").value,
    });
    el.settings.close();
    await refresh();
  } catch (error) {
    settingsError = String(error);
    await updateWarnings();
  }
});

refresh();
