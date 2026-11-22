// What an agent reads and does in a program's web view (`bateri view`).
//
// This script lives in a content world of its own, named for the agent: the page's scripts cannot
// see it, its variables or its functions, and it cannot be told by them what to answer. It runs
// in every frame of the web view and acts only in the one the shell page holds, the program's:
// the frame whose parent is the top document. It tells the app that it is there with one message
// to a handler that exists in this world alone, so that the app has the frame to call.
//
// The app calls `window.__bateriAgent.snapshot()` and the actions, and gets back text: a record
// of lines of tab-separated words, a word that is text written by `encodeURIComponent` so that
// nothing of the page can end one early (`bt-shell-common`, `agent/view/web.rs` says the format),
// or `ok` / `err` and a code for an action. An element is told by a number this script gives it
// the first time it sees it and keeps for as long as the document lives; the page cannot see it.
//
// What is read is what a screen reader reads, by the same rules as a native page's: an element
// that holds others (a dialog, a list, a table, a row that has buttons in it) is one item with
// what is in it read under it (`parent`), a page behind an open modal (`aria-modal`, a `<dialog>`
// shown as a modal) is not read, and what is `inert` is not read at all.
//
// Nothing here moves the window or the app's keyboard: the events are the page's own, dispatched
// to its elements, and the reading is of the document as it is. (The app, not this script, keeps
// the pasteboard: a page's Copy button runs as a person's gesture.)
(() => {
  "use strict";
  if (window === window.top || window.parent !== window.top || window.__bateriAgent) return;

  const ITEMS_MOST = 300;
  const WORDS_MOST = 200;
  const SCROLLS_MOST = 100;
  // The fields are told of to a most too: each is measured and asked what covers it, and a table of
  // thousands of inputs would make a look longer than the call may take.
  const FIELDS_MOST = 200;
  // The elements one look visits at the most, however many are told of: the look is bounded.
  const WALK_MOST = 5000;

  // ─── Numbers ──────────────────────────────────────────────────────────────────────────────────
  const numbers = new WeakMap();
  const elements = new Map();
  let counter = 0;
  const numberOf = (el) => {
    let n = numbers.get(el);
    if (!n) {
      n = ++counter;
      numbers.set(el, n);
    }
    // An element that left the document and came back has its number, and is found by it again.
    if (!elements.has(n)) elements.set(n, new WeakRef(el));
    return n;
  };
  // What is gone from the document is let go of: a page that draws again and again would otherwise
  // make the map as long as the document has lived.
  const prune = () => {
    for (const [n, ref] of elements) {
      const el = ref.deref();
      if (!el || !el.isConnected) elements.delete(n);
    }
  };
  const find = (n) => {
    const el = elements.get(n)?.deref();
    return el && el.isConnected ? el : null;
  };

  // ─── Rest ─────────────────────────────────────────────────────────────────────────────────────
  // What the document does is what the agent waits for: a page is at rest when it has stood still
  // for a while.
  let changed = performance.now();
  new MutationObserver(() => {
    changed = performance.now();
  }).observe(document, { subtree: true, childList: true, attributes: true, characterData: true });

  // ─── Words ────────────────────────────────────────────────────────────────────────────────────
  const enc = encodeURIComponent;
  const clean = (s, n = WORDS_MOST) => (s || "").replace(/\s+/g, " ").trim().slice(0, n);
  const flag = (b) => (b ? "1" : "0");
  const box = (el) => {
    const r = el.getBoundingClientRect();
    return [r.x, r.y, r.width, r.height].map((v) => Math.round(v * 10) / 10).join("\t");
  };

  const shown = (el) => {
    if (el.checkVisibility) {
      if (!el.checkVisibility({ checkOpacity: true, checkVisibilityCSS: true })) return false;
    } else {
      const s = getComputedStyle(el);
      if (s.display === "none" || s.visibility === "hidden") return false;
    }
    const r = el.getBoundingClientRect();
    return r.width > 0 || r.height > 0;
  };

  const labelOf = (el) => {
    const aria = el.getAttribute("aria-label");
    if (aria) return clean(aria);
    const by = el.getAttribute("aria-labelledby");
    if (by) {
      const words = by
        .split(/\s+/)
        .map((id) => document.getElementById(id)?.textContent || "")
        .join(" ");
      if (clean(words)) return clean(words);
    }
    if (el.labels && el.labels.length) return clean(el.labels[0].textContent);
    return clean(el.title || "");
  };

  // ─── What an element is ───────────────────────────────────────────────────────────────────────
  const BUTTONS = new Set(["button", "submit", "reset", "image"]);
  const NOT_TEXT = new Set(["button", "submit", "reset", "image", "checkbox", "radio", "range", "hidden", "file", "color"]);

  const fieldKind = (el) => {
    const t = el.tagName;
    if (t === "TEXTAREA") return "lines";
    if (t === "SELECT") return "line";
    if (t === "INPUT") {
      const type = (el.getAttribute("type") || "text").toLowerCase();
      if (type === "password") return "secret";
      return NOT_TEXT.has(type) ? null : "line";
    }
    if (el.isContentEditable && el.getAttribute("contenteditable") !== "false" && el.parentElement && !el.parentElement.isContentEditable) {
      return "lines";
    }
    return null;
  };

  // ARIA's words that the reading says in another: its grids are tables, its cells of a grid are
  // cells. What is only for the page's own markup says nothing.
  const ALIAS = { grid: "table", treegrid: "table", gridcell: "cell", rowheader: "cell", menuitemcheckbox: "menuitem", menuitemradio: "menuitem", img: "image" };
  const SILENT = new Set(["none", "presentation", "generic", "separator", "rowgroup"]);
  // The roles of ARIA that hold other elements and which the reading has no word for: a group that
  // says what the page called it.
  const GROUPS = new Set([
    "application", "article", "banner", "complementary", "contentinfo", "directory", "document", "feed",
    "figure", "form", "main", "menu", "menubar", "navigation", "radiogroup", "region", "search",
    "tabpanel", "toolbar", "tooltip",
  ]);
  // Named by the label alone, and what is in them is read under them.
  const HOLDERS = new Set(["dialog", "alertdialog", "group", "list", "listbox", "table", "tree", "tablist"]);
  // Named by what is in them, unless controls of their own are, and then they are as a holder is.
  const MIXED = new Set(["listitem", "row", "cell", "columnheader", "treeitem", "tab", "option", "status", "alert"]);
  const HOLDS = "a[href],button,input,select,textarea,summary,td,th,tr,li,[role],[onclick],[tabindex],[contenteditable]";
  const holdsMore = (el) => {
    for (const child of el.children) if (child.matches(HOLDS) || child.querySelector(HOLDS)) return true;
    return false;
  };
  const plainMarkup = (el, tag) => {
    const holder = el.closest(tag);
    const given = holder && holder.getAttribute("role");
    return !!given && SILENT.has(given.trim().split(/\s+/)[0].toLowerCase());
  };

  const givenRole = (el) => {
    const given = el.getAttribute("role");
    return given ? given.trim().split(/\s+/)[0].toLowerCase() : "";
  };

  const roleOf = (el) => {
    const given = givenRole(el);
    if (given) {
      if (given === "heading") return "heading" + Math.min(6, Math.max(1, parseInt(el.getAttribute("aria-level"), 10) || 2));
      if (SILENT.has(given)) return null;
      if (GROUPS.has(given)) return "group";
      return ALIAS[given] || given;
    }
    const t = el.tagName.toLowerCase();
    if (t === "a") return el.hasAttribute("href") ? "link" : null;
    if (t === "button" || t === "summary") return "button";
    if (t === "input") {
      const type = (el.getAttribute("type") || "text").toLowerCase();
      if (BUTTONS.has(type)) return "button";
      if (type === "checkbox" || type === "radio") return type;
      if (type === "range") return "slider";
      return null;
    }
    if (/^h[1-6]$/.test(t)) return "heading" + t[1];
    if (t === "img") return el.alt ? "image" : null;
    if (t === "progress") return "progress";
    if (t === "dialog") return "dialog";
    if (t === "ul" || t === "ol") return plainMarkup(el, "ul,ol") ? null : "list";
    if (t === "li") return el.parentElement && plainMarkup(el, "ul,ol") ? null : "listitem";
    if (t === "table") return plainMarkup(el, "table") ? null : "table";
    if (t === "tr" || t === "td" || t === "th") {
      if (plainMarkup(el, "table")) return null;
      if (t === "tr") return "row";
      return t === "th" && el.getAttribute("scope") !== "row" ? "columnheader" : "cell";
    }
    return null;
  };

  // What the page calls a kind of element the reading has no word for: its own, or the ARIA name of
  // the group it was.
  const roleDescriptionOf = (el, role) => {
    const own = clean(el.getAttribute("aria-roledescription") || "");
    if (own) return own;
    const given = givenRole(el);
    return role === "group" && GROUPS.has(given) ? given : "";
  };

  const describedBy = (el) => {
    const own = clean(el.getAttribute("aria-description") || "");
    if (own) return own;
    const by = el.getAttribute("aria-describedby");
    if (!by) return "";
    return clean(
      by
        .split(/\s+/)
        .map((id) => document.getElementById(id)?.textContent || "")
        .join(" "),
    );
  };

  const nameOf = (el) => {
    const label = labelOf(el);
    if (label) return label;
    const t = el.tagName;
    if (t === "IMG") return clean(el.alt);
    if (t === "INPUT") {
      const type = (el.getAttribute("type") || "text").toLowerCase();
      if (BUTTONS.has(type)) return clean(el.value || el.alt || "");
    }
    return clean(el.innerText || el.textContent || "");
  };

  // The rows of a table and the peers of an element: asked once for a look, and let go of for the next.
  let rowsets = new Map();
  let peersets = new Map();
  const TABLES = "[role=table],[role=grid],[role=treegrid],table,[role=list],ul,ol,[role=listbox],[role=tree]";
  const ROWS = "[role=row],tr,[role=listitem],li,[role=option],[role=treeitem]";
  const CELLS = "td,th,[role=cell],[role=gridcell],[role=columnheader],[role=rowheader]";
  const whole = (n) => {
    const number = parseInt(n, 10);
    return Number.isFinite(number) && number >= 1 ? number : null;
  };
  const rowsOf = (table) => {
    let rows = rowsets.get(table);
    if (!rows) {
      rows = [...table.querySelectorAll(ROWS)].filter((row) => row.parentElement && row.parentElement.closest(TABLES) === table);
      rowsets.set(table, rows);
    }
    return rows;
  };
  // The place of a row in what it is a row of, from 1: what the page says (`aria-rowindex`), a table
  // row's own place in the table, or its place among the rows.
  const rowIndexOf = (row) => {
    const said = whole(row.getAttribute("aria-rowindex"));
    if (said) return said;
    if (row.tagName === "TR" && row.rowIndex >= 0) return row.rowIndex + 1;
    const table = row.parentElement && row.parentElement.closest(TABLES);
    return table ? rowsOf(table).indexOf(row) + 1 : 0;
  };
  const cellsOf = (row) => [...row.querySelectorAll(CELLS)].filter((cell) => cell.parentElement && cell.parentElement.closest("tr,[role=row]") === row);
  const SET_ROLES = new Set(["listitem", "option", "tab", "treeitem"]);
  const peers = (el, role) => {
    const parent = el.parentElement;
    if (!parent) return [];
    let byRole = peersets.get(parent);
    if (!byRole) {
      byRole = {};
      peersets.set(parent, byRole);
    }
    return byRole[role] || (byRole[role] = [...parent.children].filter((child) => roleOf(child) === role));
  };
  const treeLevel = (el) => {
    let level = 1;
    for (let up = el.parentElement; up && givenRole(up) !== "tree"; up = up.parentElement) if (givenRole(up) === "treeitem") level++;
    return level;
  };

  // How an element stands: a comma list of words, some of them `name=number`.
  const stateOf = (el, role) => {
    const flags = [];
    const aria = (name) => el.getAttribute(name);
    if (role === "checkbox" || role === "radio" || role === "switch" || aria("aria-checked") !== null || aria("aria-pressed") !== null) {
      if (aria("aria-checked") === "mixed" || aria("aria-pressed") === "mixed" || el.indeterminate === true) flags.push("mixed");
      else {
        const on = el.checked !== undefined ? el.checked : aria("aria-checked") === "true" || aria("aria-pressed") === "true";
        flags.push(on ? "checked" : "unchecked");
      }
    }
    if (aria("aria-selected") === "true" || el.selected === true) flags.push("selected");
    if (aria("aria-expanded") !== null) flags.push(aria("aria-expanded") === "true" ? "expanded" : "collapsed");
    else if (el.tagName === "DETAILS" || (el.tagName === "SUMMARY" && el.parentElement)) {
      const details = el.tagName === "DETAILS" ? el : el.parentElement;
      if (details.tagName === "DETAILS") flags.push(details.open ? "expanded" : "collapsed");
    }
    if (el.disabled === true || aria("aria-disabled") === "true") flags.push("disabled");
    const level = whole(aria("aria-level")) || (role === "treeitem" ? treeLevel(el) : 0);
    if (level && !role.startsWith("heading")) flags.push("level=" + level);
    if (SET_ROLES.has(role)) {
      let size = whole(aria("aria-setsize"));
      let at = whole(aria("aria-posinset"));
      if (!size || !at) {
        const all = peers(el, role);
        if (all.length > 1) {
          size = size || all.length;
          at = at || all.indexOf(el) + 1;
        }
      }
      if (size) flags.push("setsize=" + size);
      if (at) flags.push("posinset=" + at);
    }
    if (role === "table") {
      const rows = aria("aria-rowcount") === null ? null : parseInt(aria("aria-rowcount"), 10);
      const columns = aria("aria-colcount") === null ? null : parseInt(aria("aria-colcount"), 10);
      const all = rows > 0 && columns > 0 ? [] : rowsOf(el);
      flags.push("rowcount=" + (rows > 0 ? rows : all.length));
      flags.push("colcount=" + (columns > 0 ? columns : all.length ? cellsOf(all[0]).length : 0));
    }
    if (role === "row") {
      const at = rowIndexOf(el);
      if (at) flags.push("rowindex=" + at);
    }
    if (role === "cell" || role === "columnheader") {
      const row = el.parentElement && el.parentElement.closest("tr,[role=row]");
      const at = whole(aria("aria-colindex")) || (el.tagName === "TD" || el.tagName === "TH" ? el.cellIndex + 1 : row ? cellsOf(row).indexOf(el) + 1 : 0);
      if (at) flags.push("colindex=" + at);
    }
    const sort = aria("aria-sort");
    if (sort === "ascending" || sort === "descending") flags.push("sort=" + sort);
    let value = "-";
    if (role === "slider" || role === "progress") value = "+" + enc(clean(String(el.value ?? aria("aria-valuenow") ?? "")));
    return [flags.join(","), value];
  };

  const clickable = (el) => {
    if (el.hasAttribute("onclick")) return true;
    if (el.hasAttribute("tabindex") && el.tabIndex >= 0) return true;
    const style = getComputedStyle(el);
    if (style.cursor !== "pointer") return false;
    // The cursor is inherited: the element that sets it is the one that is clicked.
    const parent = el.parentElement;
    return !parent || getComputedStyle(parent).cursor !== "pointer";
  };

  const scrollable = (el) => {
    if (el === document.scrollingElement || el === document.documentElement || el === document.body) return false;
    if (el.scrollHeight <= el.clientHeight + 1) return false;
    const overflow = getComputedStyle(el).overflowY;
    return overflow === "auto" || overflow === "scroll" || overflow === "overlay";
  };

  const percentOf = (el) => {
    const most = el.scrollHeight - el.clientHeight;
    return most <= 0 ? 0 : Math.max(0, Math.min(100, Math.round((el.scrollTop / most) * 100)));
  };

  const scrollRecord = (el) =>
    ["scroll", numberOf(el), percentOf(el), flag(el.scrollTop > 0), flag(el.scrollTop + el.clientHeight < el.scrollHeight - 1), box(el)].join("\t");

  const covered = (el) => {
    const r = el.getBoundingClientRect();
    const x = r.x + r.width / 2;
    const y = r.y + r.height / 2;
    if (x < 0 || y < 0 || x > innerWidth || y > innerHeight) return false;
    const top = document.elementFromPoint(x, y);
    return !!top && !(el.contains(top) || top.contains(el));
  };

  // ─── The modal ────────────────────────────────────────────────────────────────────────────────
  // The element that shuts the rest of the page out, if one is open: a dialog that says it is modal,
  // or a `<dialog>` shown as one. Of several, the last in the document is the one in front.
  const modalOf = () => {
    let found = null;
    for (const el of document.querySelectorAll('[aria-modal="true"], dialog[open]')) {
      if (el.inert || !shown(el)) continue;
      if (el.tagName === "DIALOG") {
        let modal = true;
        try {
          modal = el.matches(":modal");
        } catch (_) {
          // A web view that does not know `:modal` takes an open `<dialog>` for one.
        }
        if (!modal) continue;
      } else if (roleOf(el) !== "dialog" && roleOf(el) !== "alertdialog") {
        continue;
      }
      found = el;
    }
    return found;
  };

  // ─── The look ─────────────────────────────────────────────────────────────────────────────────
  const snapshot = () => {
    const items = [];
    const fields = [];
    const scrolls = [];
    let more = false;
    let visited = 0;
    rowsets = new Map();
    peersets = new Map();
    const modal = modalOf();
    // How many things the page behind the modal would have said: not in the record.
    let behind = 0;
    const spent = () => {
      if (++visited <= WALK_MOST) return false;
      more = true;
      return true;
    };
    // Whether the element is of what the modal shows (the page behind is not).
    const inFront = (el) => !modal || modal.contains(el);
    const optional = (words) => (words ? "+" + enc(words) : "-");

    // An item is told, under the item that holds it where one does (`parent`, the place of that in the
    // record's items, or -1); what is behind the modal is only counted. The place of the item, or -1.
    const tell = (el, role, words, flags, value, parent) => {
      if (!inFront(el)) {
        behind++;
        return -1;
      }
      const about = role === "text" ? "" : describedBy(el);
      const keys = role === "text" ? "" : clean(el.getAttribute("aria-keyshortcuts") || "");
      const kind = role === "text" ? "" : roleDescriptionOf(el, role);
      items.push(
        ["item", numberOf(el), enc(role), enc(words), flags, value, box(el), parent < 0 ? "-" : parent, optional(kind), optional(about), optional(keys)].join("\t"),
      );
      return items.length - 1;
    };

    const pushField = (el, kind) => {
      if (!inFront(el)) return;
      if (fields.length >= FIELDS_MOST) {
        more = true;
        return;
      }
      const t = el.tagName;
      const value =
        kind === "secret"
          ? "-"
          : "+" + enc(t === "SELECT" ? clean(el.selectedOptions[0]?.text || "", 400) : el.isContentEditable && t !== "INPUT" && t !== "TEXTAREA" ? clean(el.textContent, 400) : (el.value || "").slice(0, 400));
      fields.push(["field", numberOf(el), kind, enc(labelOf(el)), enc(clean(el.getAttribute("placeholder") || "")), value, flag(covered(el)), box(el)].join("\t"));
    };

    // Words of the element's own, which none of its children say.
    const ownText = (el, parent) => {
      let own = "";
      for (const node of el.childNodes) if (node.nodeType === 3) own += " " + node.nodeValue;
      own = clean(own);
      if (own) tell(el, "text", own, "", "-", parent);
    };

    const walk = (el, parent) => {
      // Past the most there are no more words, but the fields and the boxes that scroll are still
      // what an agent acts on: they are told of to the end.
      if (items.length >= ITEMS_MOST) {
        more = true;
        walkInside(el);
        return;
      }
      if (spent() || el.nodeType !== 1) return;
      const t = el.tagName;
      if (t === "SCRIPT" || t === "STYLE" || t === "NOSCRIPT" || t === "TEMPLATE") return;
      if (!shown(el) || el.inert) return;

      const kind = fieldKind(el);
      if (kind) {
        pushField(el, kind);
        return;
      }
      // What holds the modal is of the page behind it: it says nothing, and what is in it is read.
      if (modal && el !== modal && el.contains(modal)) {
        ownText(el, parent);
        for (const child of el.children) walk(child, parent);
        return;
      }
      if (scrolls.length < SCROLLS_MOST && inFront(el) && scrollable(el)) scrolls.push(scrollRecord(el));
      let role = roleOf(el);
      // A box that answers a click and holds a dialog (a scrim) is not a thing to press: what is in
      // it is read.
      if (!role && clickable(el) && !el.querySelector('dialog,[role="dialog"],[role="alertdialog"]') && clean(el.innerText || el.getAttribute("aria-label") || "")) role = "clickable";
      if (role) {
        const holder = HOLDERS.has(role) || (MIXED.has(role) && holdsMore(el));
        const [flags, value] = stateOf(el, role);
        const at = tell(el, role, holder ? labelOf(el) : nameOf(el), flags, value, parent);
        if (!holder) {
          // What is inside it is its name: only the boxes and fields that scroll or type are its own.
          if (!role.startsWith("heading") && role !== "image") for (const child of el.children) walkInside(child);
          return;
        }
        const under = at >= 0 ? at : parent;
        ownText(el, under);
        for (const child of el.children) walk(child, under);
        return;
      }
      ownText(el, parent);
      for (const child of el.children) walk(child, parent);
    };

    // The fields and the boxes that scroll inside what is already named: no more words of it.
    const walkInside = (el) => {
      if (spent() || el.nodeType !== 1 || !shown(el) || el.inert) return;
      const kind = fieldKind(el);
      if (kind) {
        pushField(el, kind);
        return;
      }
      if (scrolls.length < SCROLLS_MOST && inFront(el) && scrollable(el)) scrolls.push(scrollRecord(el));
      for (const child of el.children) walkInside(child);
    };

    const root = document.body || document.documentElement;
    if (root) walk(root, -1);
    prune();
    const page = document.scrollingElement;
    if (!modal && page && page.scrollHeight > page.clientHeight + 1) {
      const most = page.scrollHeight - page.clientHeight;
      scrolls.unshift(
        [
          "scroll",
          numberOf(page),
          Math.max(0, Math.min(100, Math.round((page.scrollTop / most) * 100))),
          flag(page.scrollTop > 0),
          flag(page.scrollTop + page.clientHeight < page.scrollHeight - 1),
          [0, 0, innerWidth, innerHeight].join("\t"),
        ].join("\t"),
      );
    }

    const active = document.activeElement;
    const focus = active && active !== document.body && inFront(active) ? numbers.get(active) || 0 : 0;
    const images = [...document.images].filter((img) => !img.complete && img.loading !== "lazy").length;
    const out = [
      "v2",
      ["rest", Math.round(performance.now() - changed), flag(images > 0), flag(document.readyState !== "complete")].join("\t"),
      "focus\t" + focus,
      ...(modal ? ["behind\t" + behind] : []),
      ...items,
      ...fields,
      ...scrolls,
    ];
    if (more) out.push("note\t" + enc(`the page is larger than a reading lists (${ITEMS_MOST} elements, ${FIELDS_MOST} fields, ${WALK_MOST} visited): the rest are not listed`));
    return out.join("\n");
  };

  // ─── What is done ─────────────────────────────────────────────────────────────────────────────
  const ok = () => "ok";
  const err = (code, detail = "") => "err\t" + code + "\t" + enc(detail);
  const describe = (el) => (el ? clean(el.getAttribute("aria-label") || el.innerText || el.tagName.toLowerCase(), 40) : "something");
  const FOCUSABLE = "a[href],button,input,select,textarea,summary,[tabindex],[contenteditable]";
  const field = (el) => fieldKind(el) !== null;

  const click = (n, times) => {
    const el = find(n);
    if (!el) return err("gone");
    if (!shown(el)) return err("hidden");
    el.scrollIntoView({ block: "center", inline: "center" });
    const r = el.getBoundingClientRect();
    const x = r.x + r.width / 2;
    const y = r.y + r.height / 2;
    const top = document.elementFromPoint(x, y);
    if (!top || !(el.contains(top) || top.contains(el))) return err("covered", describe(top));
    for (let count = 1; count <= times; count++) {
      const init = { bubbles: true, cancelable: true, composed: true, view: window, clientX: x, clientY: y, button: 0, buttons: 1, detail: count };
      top.dispatchEvent(new PointerEvent("pointerdown", { ...init, pointerType: "mouse", isPrimary: true }));
      top.dispatchEvent(new MouseEvent("mousedown", init));
      const target = el.matches(FOCUSABLE) ? el : el.closest(FOCUSABLE);
      if (target) target.focus({ preventScroll: true });
      const up = { ...init, buttons: 0 };
      top.dispatchEvent(new PointerEvent("pointerup", { ...up, pointerType: "mouse", isPrimary: true }));
      top.dispatchEvent(new MouseEvent("mouseup", up));
      top.dispatchEvent(new MouseEvent("click", up));
    }
    if (times === 2) top.dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true, composed: true, view: window, clientX: x, clientY: y, detail: 2 }));
    return ok();
  };

  const input = (el, data, type = "insertText") => el.dispatchEvent(new InputEvent("input", { bubbles: true, composed: true, inputType: type, data }));

  const fill = (n, value) => {
    const el = find(n);
    if (!el) return err("gone");
    if (!shown(el)) return err("hidden");
    const t = el.tagName;
    if (t === "SELECT") {
      const options = [...el.options];
      const wanted = value.trim().toLowerCase();
      const option = options.find((o) => o.text.trim() === value || o.value === value) || options.find((o) => o.text.trim().toLowerCase() === wanted);
      if (!option) return err("nooption", options.slice(0, 20).map((o) => clean(o.text, 40)).join("|"));
      el.focus({ preventScroll: true });
      el.value = option.value;
      input(el, null, "insertReplacementText");
      el.dispatchEvent(new Event("change", { bubbles: true }));
      return ok();
    }
    if (t === "INPUT" || t === "TEXTAREA") {
      if (fieldKind(el) === null) return err("notfield");
      el.focus({ preventScroll: true });
      const proto = t === "INPUT" ? HTMLInputElement.prototype : HTMLTextAreaElement.prototype;
      Object.getOwnPropertyDescriptor(proto, "value").set.call(el, value);
      input(el, value);
      el.dispatchEvent(new Event("change", { bubbles: true }));
      return ok();
    }
    if (fieldKind(el) === "lines") {
      el.focus({ preventScroll: true });
      document.execCommand("selectAll");
      if (!document.execCommand("insertText", false, value)) {
        el.textContent = value;
        input(el, value);
      }
      return ok();
    }
    return err("notfield");
  };

  // Text at the caret of the element the keyboard is in.
  const insert = (el, text) => {
    const t = el.tagName;
    if (t === "INPUT" || t === "TEXTAREA") {
      try {
        const start = el.selectionStart ?? el.value.length;
        const end = el.selectionEnd ?? el.value.length;
        el.setRangeText(text, start, end, "end");
      } catch (_) {
        // A kind of input that has no caret (a number, an email): the text goes at the end.
        const proto = t === "INPUT" ? HTMLInputElement.prototype : HTMLTextAreaElement.prototype;
        Object.getOwnPropertyDescriptor(proto, "value").set.call(el, el.value + text);
      }
      input(el, text);
      return true;
    }
    if (el.isContentEditable) {
      if (!document.execCommand("insertText", false, text)) {
        el.textContent += text;
        input(el, text);
      }
      return true;
    }
    return false;
  };

  const type = (value) => {
    const el = document.activeElement;
    if (!el || el === document.body || !field(el)) return err("nofocus");
    return insert(el, value) ? ok() : err("nofocus");
  };

  const erase = (el, forward) => {
    if (el.tagName === "INPUT" || el.tagName === "TEXTAREA") {
      const start = el.selectionStart ?? 0;
      const end = el.selectionEnd ?? 0;
      const [from, to] = start !== end ? [start, end] : forward ? [start, Math.min(el.value.length, end + 1)] : [Math.max(0, start - 1), end];
      if (from === to) return;
      try {
        el.setRangeText("", from, to, "end");
      } catch (_) {
        return;
      }
      input(el, null, forward ? "deleteContentForward" : "deleteContentBackward");
    } else if (el.isContentEditable) {
      document.execCommand(forward ? "forwardDelete" : "delete");
    }
  };

  const focusNext = (backwards) => {
    const all = [...document.querySelectorAll(FOCUSABLE)].filter((el) => !el.disabled && el.tabIndex >= 0 && shown(el));
    if (!all.length) return;
    const at = all.indexOf(document.activeElement);
    const next = at < 0 ? (backwards ? all.length - 1 : 0) : (at + (backwards ? -1 : 1) + all.length) % all.length;
    all[next].focus();
  };

  // What the page does for a key it did not take: the few things a person's keys do that no event
  // does by itself.
  const keyed = (target, k) => {
    const { key, shiftKey, ctrlKey, altKey, metaKey } = k;
    const plain = !ctrlKey && !altKey && !metaKey;
    const t = target.tagName;
    if (key === "Enter" && plain) {
      if (t === "TEXTAREA" || (target.isContentEditable && t !== "INPUT")) return insert(target, "\n");
      if (t === "INPUT" && !BUTTONS.has((target.getAttribute("type") || "text").toLowerCase()) && target.form) return target.form.requestSubmit();
      if (target.matches("a[href],button,summary,[role=button],[role=link],input[type=button],input[type=submit]")) return target.click();
      return;
    }
    if (key === " " && plain && target.matches("button,summary,[role=button],input[type=checkbox],input[type=radio],input[type=button],input[type=submit]")) return target.click();
    if (key === "Tab" && !ctrlKey && !altKey && !metaKey) return focusNext(shiftKey);
    if (key === "Backspace" && plain && field(target)) return erase(target, false);
    if (key === "Delete" && plain && field(target)) return erase(target, true);
    if (key.length === 1 && plain && field(target)) insert(target, key);
  };

  const press = (list) => {
    for (const [key, code, shiftKey, ctrlKey, altKey, metaKey] of list) {
      const target = document.activeElement || document.body;
      const init = { key, code, shiftKey, ctrlKey, altKey, metaKey, bubbles: true, cancelable: true, composed: true, view: window };
      const down = target.dispatchEvent(new KeyboardEvent("keydown", init));
      if (down) {
        if (key.length === 1 && !ctrlKey && !altKey && !metaKey) target.dispatchEvent(new KeyboardEvent("keypress", { ...init, charCode: key.charCodeAt(0) }));
        keyed(target, init);
      }
      target.dispatchEvent(new KeyboardEvent("keyup", init));
    }
    return ok();
  };

  const scroll = (n, way, screens) => {
    const el = find(n);
    if (!el) return err("gone");
    const box = el === document.scrollingElement || el === document.documentElement || el === document.body ? document.scrollingElement : el;
    const view = box === document.scrollingElement ? innerHeight : box.clientHeight;
    const most = box.scrollHeight - view;
    const to = (top) => {
      box.scrollTop = Math.max(0, Math.min(most, top));
    };
    if (way === "top") to(0);
    else if (way === "bottom") to(most);
    else to(box.scrollTop + (way === "down" ? 1 : -1) * Math.max(1, view * 0.9) * screens);
    return ok();
  };

  // Brings an element, or the row of the list, the table or the outline it is that the reading says is
  // row `index` (from 1: what the page says its place is, as the reading prints it), into view: the page
  // scrolls to it. A row the page has not built (a list that builds what is in view) is no row; the
  // detail says how many it has built.
  const reveal = (n, index) => {
    const el = find(n);
    if (!el) return err("gone");
    let target = el;
    if (index) {
      rowsets = new Map();
      const rows = rowsOf(el);
      target = rows.find((row) => rowIndexOf(row) === index);
      if (!target) return err("norow", String(rows.length));
    }
    if (!shown(target)) return err("hidden");
    target.scrollIntoView({ block: "center", inline: "nearest" });
    return ok();
  };

  window.__bateriAgent = Object.freeze({ snapshot, click, fill, type, press, scroll, reveal });

  // Tell the app the frame is here, once the document can be read.
  const ready = () => {
    try {
      window.webkit.messageHandlers.bateriAgent.postMessage("ready");
    } catch (_) {
      // The handler is not in this frame's world: nobody is listening.
    }
  };
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", ready, { once: true });
  else ready();
})();
