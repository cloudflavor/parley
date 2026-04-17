import fs from "node:fs/promises";
import path from "node:path";
import markdownit from "markdown-it";

const docsDir = path.resolve(process.cwd(), "..", "docs");
const outDir = path.resolve(process.cwd(), "static", "generated");
const outFile = path.join(outDir, "docs.json");

const preferredOrder = ["overview", "quickstart", "review-workflow", "mcp"];
const md = markdownit({
  html: false,
  linkify: false,
  typographer: false,
  breaks: false,
  langPrefix: "language-",
});

function headingId(text) {
  return text
    .toLowerCase()
    .replace(/[^a-z0-9\s-]/g, "")
    .trim()
    .replace(/\s+/g, "-");
}

const defaultHeadingOpen =
  md.renderer.rules.heading_open ??
  ((tokens, idx, options, _env, self) => self.renderToken(tokens, idx, options));

md.renderer.rules.heading_open = (tokens, idx, options, env, self) => {
  const token = tokens[idx];
  if (token.tag === "h1" || token.tag === "h2" || token.tag === "h3") {
    const inline = tokens[idx + 1];
    if (inline?.type === "inline") {
      token.attrSet("id", headingId(inline.content));
    }
  }
  return defaultHeadingOpen(tokens, idx, options, env, self);
};

function slugFromFile(name) {
  return name.replace(/\.md$/i, "");
}

function titleFromMarkdown(markdown, slug) {
  const match = markdown.match(/^#\s+(.+)$/m);
  return match ? match[1].trim() : slug;
}

function summaryFromMarkdown(markdown) {
  const lines = markdown
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0 && !line.startsWith("#"));
  return lines[0] ?? "";
}

function collectHeadings(markdown) {
  return markdown
    .split("\n")
    .map((line) => line.match(/^(#{1,3})\s+(.+)$/))
    .filter(Boolean)
    .map(([, hashes, text]) => ({
      depth: hashes.length,
      text: text.trim(),
      id: headingId(text),
    }));
}

function markdownToHtml(markdown) {
  return md.render(markdown);
}

async function main() {
  const entries = (await fs.readdir(docsDir))
    .filter((name) => name.endsWith(".md"))
    .sort((a, b) => {
      const sa = slugFromFile(a);
      const sb = slugFromFile(b);
      const ia = preferredOrder.indexOf(sa);
      const ib = preferredOrder.indexOf(sb);
      if (ia !== -1 || ib !== -1) {
        return (ia === -1 ? Number.MAX_SAFE_INTEGER : ia) - (ib === -1 ? Number.MAX_SAFE_INTEGER : ib);
      }
      return sa.localeCompare(sb);
    });

  const docs = [];
  for (const fileName of entries) {
    const slug = slugFromFile(fileName);
    const filePath = path.join(docsDir, fileName);
    const markdown = await fs.readFile(filePath, "utf8");
    docs.push({
      slug,
      title: titleFromMarkdown(markdown, slug),
      summary: summaryFromMarkdown(markdown),
      headings: collectHeadings(markdown),
      html: markdownToHtml(markdown),
    });
  }

  await fs.mkdir(outDir, { recursive: true });
  await fs.writeFile(outFile, JSON.stringify(docs, null, 2));
  console.log(`generated ${docs.length} docs into ${path.relative(process.cwd(), outFile)}`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
