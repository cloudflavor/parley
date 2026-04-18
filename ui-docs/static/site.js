const THEME_KEY = "parley-docs-theme";

function safeJsonParse(value) {
  try {
    return JSON.parse(value);
  } catch {
    return null;
  }
}

function getInitialTheme() {
  const saved = localStorage.getItem(THEME_KEY);
  if (saved === "light" || saved === "dark") {
    return saved;
  }
  return "dark";
}

function setTheme(theme) {
  document.documentElement.dataset.theme = theme;
  for (const button of document.querySelectorAll("[data-theme-toggle]")) {
    button.textContent = theme === "dark" ? "Switch to Light" : "Switch to Dark";
    button.setAttribute("aria-label", button.textContent);
  }
}

function installThemeToggle() {
  setTheme(getInitialTheme());

  for (const button of document.querySelectorAll("[data-theme-toggle]")) {
    button.addEventListener("click", () => {
      const next = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
      setTheme(next);
      localStorage.setItem(THEME_KEY, next);
    });
  }
}

async function loadDocs() {
  if (Array.isArray(window.__PARLEY_DOCS__)) {
    return window.__PARLEY_DOCS__;
  }
  throw new Error("Generated docs payload was not injected into the page");
}

function escapeHtml(value) {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function documentUrl(slug) {
  return `/docs/${slug}`;
}

function depthTwoHeadings(doc) {
  return doc.headings.filter((heading) => heading.depth === 2);
}

function headingCount(doc) {
  return doc.headings.filter((heading) => heading.depth >= 2).length;
}

function readTimeMinutes(doc) {
  const approxWords = `${doc.title} ${doc.summary} ${doc.headings.map((entry) => entry.text).join(" ")}`.trim().split(/\s+/)
    .filter(Boolean).length * 22;
  return Math.max(1, Math.round(approxWords / 180));
}

window.ParleyDocs = {
  depthTwoHeadings,
  documentUrl,
  escapeHtml,
  headingCount,
  installThemeToggle,
  loadDocs,
  readTimeMinutes,
  safeJsonParse,
};
