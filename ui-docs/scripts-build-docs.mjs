import fs from "node:fs/promises";
import path from "node:path";
import markdownit from "markdown-it";

const docsDir = path.resolve(process.cwd(), "..", "docs");
const outDir = path.resolve(process.cwd(), "static", "generated");
const outFile = path.join(outDir, "docs.json");
const markdownOutDir = path.join(outDir, "markdown");
const staticDir = path.resolve(process.cwd(), "static");
const homeTemplateFile = path.resolve(process.cwd(), "templates", "index.html");
const docsTemplateFile = path.resolve(process.cwd(), "templates", "docs-index.html");
const homeOutFile = path.join(staticDir, "index.html");
const docsIndexOutFile = path.join(staticDir, "docs", "index.html");
const systemCssDistDir = path.resolve(
  process.cwd(),
  "node_modules",
  "@sakun",
  "system.css",
  "dist"
);
const systemCssOutDir = path.resolve(process.cwd(), "static", "vendor", "system.css");

const preferredOrder = ["overview", "quickstart", "keybindings", "review-workflow", "mcp"];
const md = markdownit({
  html: false,
  linkify: false,
  typographer: false,
  breaks: false,
  langPrefix: "language-",
});
const homeMarkdownOutFile = path.join(markdownOutDir, "index.md");
const docsIndexMarkdownOutFile = path.join(markdownOutDir, "docs", "index.md");

function decodeHtmlEntities(text) {
  return text
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&amp;/g, "&");
}

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

function cleanInlineMarkdown(text) {
  return text
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/\*([^*]+)\*/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .trim();
}

function summaryFromHeadings(headings) {
  const topics = headings
    .filter((heading) => heading.depth === 2)
    .slice(0, 3)
    .map((heading) =>
      cleanInlineMarkdown(
        heading.text
          .replace(/^\d+\.\s*/, "")
          .replace(/\s*\([^)]*\)\s*/g, " ")
          .replace(/\s+/g, " ")
      )
    )
    .filter(Boolean);

  if (topics.length === 0) {
    return "";
  }

  if (topics.length === 1) {
    return `Covers ${topics[0]}.`;
  }

  if (topics.length === 2) {
    return `Covers ${topics[0]} and ${topics[1]}.`;
  }

  return `Covers ${topics[0]}, ${topics[1]}, and ${topics[2]}.`;
}

function summaryFromMarkdown(markdown, headings) {
  const paragraph = [];
  let inFence = false;

  for (const rawLine of markdown.split("\n")) {
    const line = rawLine.trim();

    if (line.startsWith("```")) {
      inFence = !inFence;
      if (paragraph.length > 0) {
        break;
      }
      continue;
    }

    if (inFence) {
      continue;
    }

    if (line.length === 0) {
      if (paragraph.length > 0) {
        break;
      }
      continue;
    }

    if (
      line.startsWith("#") ||
      line.startsWith("- ") ||
      line.startsWith("* ") ||
      line.startsWith(">") ||
      /^\d+\.\s/.test(line)
    ) {
      if (paragraph.length > 0) {
        break;
      }
      continue;
    }

    paragraph.push(line);
  }

  const summary = cleanInlineMarkdown(
    paragraph
    .join(" ")
  );

  if (
    summary.length > 0 &&
    !summary.endsWith(":") &&
    !summary.startsWith("This ") &&
    summary.split(/\s+/).length >= 5
  ) {
    return summary;
  }

  return summaryFromHeadings(headings);
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

function extractHomeMarkdown(template, docs) {
  const hero = template.match(
    /<section class="hero">[\s\S]*?<h1>([\s\S]*?)<\/h1>[\s\S]*?<p>\s*([\s\S]*?)\s*<\/p>[\s\S]*?<\/section>/
  );

  if (!hero) {
    throw new Error("failed to extract homepage hero copy from template");
  }

  const title = decodeHtmlEntities(hero[1].replace(/<[^>]+>/g, "").trim());
  const intro = decodeHtmlEntities(
    hero[2]
      .replace(/<[^>]+>/g, "")
      .replace(/\s+/g, " ")
      .trim()
  );
  const lines = [`# ${title}`, "", intro, "", "## Documents", ""];

  for (const doc of docs) {
    lines.push(`- [${doc.title}](/docs/${doc.slug}): ${doc.summary}`);
  }

  return `${lines.join("\n")}\n`;
}

function docsIndexMarkdown(docs) {
  const lines = [
    "# Parley Docs",
    "",
    "Workflow-focused Parley documentation for terminal review, TUI controls, and MCP integration.",
    "",
    "## Documents",
    "",
  ];

  for (const doc of docs) {
    lines.push(`- [${doc.title}](/docs/${doc.slug})`);
    lines.push(`  ${doc.summary}`);
  }

  return `${lines.join("\n")}\n`;
}

function inlineDocsPayload(template, docs) {
  return template.replace("__PARLEY_DOCS_JSON__", JSON.stringify(docs, null, 2));
}

async function listFilesRecursively(dir) {
  const entries = await fs.readdir(dir, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const entryPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await listFilesRecursively(entryPath)));
    } else if (entry.isFile()) {
      files.push(entryPath);
    }
  }
  return files;
}

async function copySystemCssAssets() {
  const files = await listFilesRecursively(systemCssDistDir);
  for (const file of files) {
    const relativePath = path.relative(systemCssDistDir, file);
    const targetPath = path.join(systemCssOutDir, relativePath);
    await fs.mkdir(path.dirname(targetPath), { recursive: true });
    await fs.copyFile(file, targetPath);
  }
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
  const docsMarkdownBySlug = new Map();
  for (const fileName of entries) {
    const slug = slugFromFile(fileName);
    const filePath = path.join(docsDir, fileName);
    const markdown = await fs.readFile(filePath, "utf8");
    const headings = collectHeadings(markdown);
    docsMarkdownBySlug.set(slug, markdown);
    docs.push({
      slug,
      title: titleFromMarkdown(markdown, slug),
      summary: summaryFromMarkdown(markdown, headings),
      headings,
      html: markdownToHtml(markdown),
    });
  }

  const [homeTemplate, docsTemplate] = await Promise.all([
    fs.readFile(homeTemplateFile, "utf8"),
    fs.readFile(docsTemplateFile, "utf8"),
  ]);
  const homeHtml = inlineDocsPayload(homeTemplate, docs);
  const docsHtml = inlineDocsPayload(docsTemplate, docs);
  const homeMarkdown = extractHomeMarkdown(homeTemplate, docs);
  const docsMarkdown = docsIndexMarkdown(docs);

  await fs.mkdir(outDir, { recursive: true });
  await fs.mkdir(markdownOutDir, { recursive: true });
  await fs.mkdir(systemCssOutDir, { recursive: true });
  await fs.mkdir(path.dirname(docsIndexOutFile), { recursive: true });
  await fs.mkdir(path.dirname(docsIndexMarkdownOutFile), { recursive: true });
  await copySystemCssAssets();
  await fs.writeFile(outFile, JSON.stringify(docs, null, 2));
  await fs.writeFile(homeOutFile, homeHtml);
  await fs.writeFile(docsIndexOutFile, docsHtml);
  await fs.writeFile(homeMarkdownOutFile, homeMarkdown);
  await fs.writeFile(docsIndexMarkdownOutFile, docsMarkdown);
  for (const doc of docs) {
    const docOutDir = path.join(staticDir, "docs", doc.slug);
    const docMarkdownOutDir = path.join(markdownOutDir, "docs");
    await fs.mkdir(docOutDir, { recursive: true });
    await fs.mkdir(docMarkdownOutDir, { recursive: true });
    await fs.writeFile(path.join(docOutDir, "index.html"), docsHtml);
    await fs.writeFile(path.join(staticDir, "docs", `${doc.slug}.html`), docsHtml);
    await fs.writeFile(path.join(docMarkdownOutDir, `${doc.slug}.md`), docsMarkdownBySlug.get(doc.slug));
  }
  console.log(`generated ${docs.length} docs into ${path.relative(process.cwd(), outFile)}`);
  console.log(`generated static docs pages from templates`);
  console.log(`generated markdown sidecars into ${path.relative(process.cwd(), markdownOutDir)}`);
  console.log(`copied system.css assets into ${path.relative(process.cwd(), systemCssOutDir)}`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
