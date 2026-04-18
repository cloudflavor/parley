(() => {
  const {
    depthTwoHeadings,
    documentUrl,
    escapeHtml,
    headingCount,
    installThemeToggle,
    loadDocs,
    readTimeMinutes,
  } = window.ParleyDocs;

  function currentSlug() {
    const prefix = "/docs/";
    if (!location.pathname.startsWith(prefix)) {
      return "overview";
    }

    const rest = location.pathname
      .slice(prefix.length)
      .replace(/\/+$/, "")
      .replace(/\.html$/i, "");
    if (!rest || rest === "index") {
      return "overview";
    }
    return rest;
  }

  function renderNav(docs, activeSlug) {
    const nav = document.getElementById("docs-nav");
    nav.innerHTML = docs
      .map(
        (doc) => `
          <li>
            <a href="${documentUrl(doc.slug)}" class="${doc.slug === activeSlug ? "active" : ""}">
              <strong>${escapeHtml(doc.title)}</strong>
              <span>${escapeHtml(doc.summary || "Open document")}</span>
            </a>
          </li>
        `
      )
      .join("");
  }

  function renderToc(doc) {
    const toc = document.getElementById("toc-list");
    const headings = depthTwoHeadings(doc);
    if (headings.length === 0) {
      toc.innerHTML = `<li class="empty-state">No section anchors in this doc.</li>`;
      return;
    }

    toc.innerHTML = headings
      .map(
        (heading) => `
          <li>
            <a href="#${escapeHtml(heading.id)}">${escapeHtml(heading.text)}</a>
          </li>
        `
      )
      .join("");
  }

  function renderDocNav(docs, currentIndex) {
    const nav = document.getElementById("doc-nav-links");
    const section = document.getElementById("doc-nav-section");
    const links = [];
    const prev = docs[currentIndex - 1];
    const next = docs[currentIndex + 1];

    if (prev) {
      links.push(`
        <li>
          <a href="${documentUrl(prev.slug)}">
            <strong>Previous</strong>
            <span>${escapeHtml(prev.title)}</span>
          </a>
        </li>
      `);
    }

    if (next) {
      links.push(`
        <li>
          <a href="${documentUrl(next.slug)}">
            <strong>Next</strong>
            <span>${escapeHtml(next.title)}</span>
          </a>
        </li>
      `);
    }

    nav.innerHTML = links.join("");
    section.hidden = links.length === 0;
  }

  function renderDoc(doc) {
    const parsed = new DOMParser().parseFromString(doc.html, "text/html");
    const topHeading = parsed.body.querySelector("h1");

    if (topHeading && topHeading.textContent.trim() === doc.title) {
      topHeading.remove();
    }

    document.title = `${doc.title} | Parley Docs`;
    document.getElementById("page-kicker").textContent = documentUrl(doc.slug);
    document.getElementById("page-title").textContent = doc.slug;
    document.getElementById("page-intro").textContent =
      doc.summary || "Workflow details and commands for this part of Parley.";
    document.getElementById("doc-meta-row").innerHTML = `
      <span class="meta-pill">${headingCount(doc)} sections</span>
      <span class="meta-pill">${readTimeMinutes(doc)} min read</span>
    `;
    document.getElementById("doc-article").innerHTML = parsed.body.innerHTML;
  }

  function installHeadingAnchors() {
    const article = document.getElementById("doc-article");
    for (const heading of article.querySelectorAll("h2[id], h3[id]")) {
      const { id } = heading;
      heading.innerHTML = `
        <a class="heading-anchor" href="#${escapeHtml(id)}" title="Link to this section">
          ${heading.innerHTML}
        </a>
      `;
    }
  }

  function installSearch(docs) {
    const input = document.getElementById("search-input");
    input.addEventListener("input", () => {
      const query = input.value.trim().toLowerCase();
      const filtered = docs.filter((doc) => {
        const haystack = `${doc.title} ${doc.summary} ${doc.headings.map((entry) => entry.text).join(" ")}`.toLowerCase();
        return haystack.includes(query);
      });
      renderNav(filtered, currentSlug());
    });
  }

  function installMobileToggle() {
    const panel = document.getElementById("side-panel");
    const button = document.getElementById("nav-toggle");
    if (!panel || !button) {
      return;
    }

    button.addEventListener("click", () => {
      const collapsed = panel.dataset.collapsed === "true";
      panel.dataset.collapsed = collapsed ? "false" : "true";
      button.textContent = collapsed ? "Hide contents" : "Show contents";
    });
  }

  async function main() {
    installThemeToggle();
    installMobileToggle();

    const article = document.getElementById("doc-article");

    try {
      const docs = await loadDocs();
      const slug = currentSlug();
      const index = docs.findIndex((doc) => doc.slug === slug);
      const doc = docs[index] ?? docs[0];

      renderNav(docs, doc.slug);
      renderDoc(doc);
      installHeadingAnchors();
      renderToc(doc);
      renderDocNav(docs, Math.max(0, docs.indexOf(doc)));
      installSearch(docs);

      if (window.hljs) {
        for (const block of article.querySelectorAll("pre code")) {
          window.hljs.highlightElement(block);
        }
      }
    } catch (error) {
      article.innerHTML = `<div class="empty-state">Failed to load docs: ${escapeHtml(error.message)}</div>`;
    }
  }

  main();
})();
