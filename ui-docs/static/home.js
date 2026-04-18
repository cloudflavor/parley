(() => {
  const { documentUrl, escapeHtml, headingCount, installThemeToggle, loadDocs, readTimeMinutes } =
    window.ParleyDocs;

  function statMarkup(label, value) {
    return `
      <article class="stat-card fade-in">
        <strong>${escapeHtml(String(value))}</strong>
        <span>${escapeHtml(label)}</span>
      </article>
    `;
  }

  function featureMarkup(title, copy) {
    return `
      <article class="feature-card fade-in">
        <h3>${escapeHtml(title)}</h3>
        <p>${escapeHtml(copy)}</p>
      </article>
    `;
  }

  function docMarkup(doc) {
    return `
      <a class="doc-card fade-in" href="${documentUrl(doc.slug)}">
        <div class="eyebrow-row">
          <span class="meta-pill">${escapeHtml(doc.slug)}</span>
          <span class="meta-pill">${headingCount(doc)} sections</span>
          <span class="meta-pill">${readTimeMinutes(doc)} min</span>
        </div>
        <h3>${escapeHtml(doc.title)}</h3>
        <p>${escapeHtml(doc.summary || "Open the document for workflow details and commands.")}</p>
      </a>
    `;
  }

  async function main() {
    installThemeToggle();

    const stats = document.getElementById("hero-stats");
    const docsGrid = document.getElementById("docs-grid");
    const featuredList = document.getElementById("feature-grid");

    try {
      const docs = await loadDocs();
      const totalSections = docs.reduce((count, doc) => count + headingCount(doc), 0);

      stats.innerHTML = [
        statMarkup("docs", docs.length),
        statMarkup("sections", totalSections),
        statMarkup("core paths", 3),
      ].join("");

      featuredList.innerHTML = [
        featureMarkup(
          "Explicit review state",
          "Threads move through open, pending, and addressed, while reviews reconcile between open, under_review, and done."
        ),
        featureMarkup(
          "Keyboard workflow",
          "The TUI is built for review from the terminal, with navigation, thread actions, review state changes, and AI commands on keys."
        ),
        featureMarkup(
          "MCP and AI control",
          "Parley exposes review tools over MCP and lets you run AI reply or refactor sessions with explicit status rules."
        ),
      ].join("");

      docsGrid.innerHTML = docs.map(docMarkup).join("");
    } catch (error) {
      stats.innerHTML = "";
      featuredList.innerHTML = "";
      docsGrid.innerHTML = `<div class="empty-state">Failed to load docs: ${escapeHtml(error.message)}</div>`;
    }
  }

  main();
})();
