const API_CATALOG_PROFILE = "https://www.rfc-editor.org/info/rfc9727";
const MARKDOWN_CONTENT_TYPE = "text/markdown; charset=utf-8";
const HOME_MARKDOWN_PATH = "/generated/markdown/index.md";
const DOCS_INDEX_MARKDOWN_PATH = "/generated/markdown/docs/index.md";
const DOCS_MARKDOWN_PREFIX = "/generated/markdown/docs";

function discoveryLinkHeader() {
  return [
    `</.well-known/api-catalog>; rel="api-catalog"; type="application/linkset+json"; title="Parley API catalog"`,
    `</docs/mcp>; rel="service-doc"; type="text/html"; title="Parley MCP integration"`,
    `</generated/docs.json>; rel="service-meta"; type="application/json"; title="Parley docs index"`,
  ].join(", ");
}

function appendDiscoveryHeaders(headers) {
  headers.set("Link", discoveryLinkHeader());
  return headers;
}

function appendVaryHeader(headers, value) {
  const existing = headers.get("Vary");
  if (!existing) {
    headers.set("Vary", value);
    return headers;
  }

  const values = existing
    .split(",")
    .map((entry) => entry.trim().toLowerCase())
    .filter(Boolean);

  if (!values.includes(value.toLowerCase())) {
    headers.set("Vary", `${existing}, ${value}`);
  }

  return headers;
}

function acceptsMarkdown(request) {
  const accept = request.headers.get("Accept");
  if (!accept) {
    return false;
  }

  return accept.split(",").some((entry) => {
    const [type, ...params] = entry.trim().toLowerCase().split(";");
    if (type !== "text/markdown") {
      return false;
    }

    const qParam = params.find((param) => param.trim().startsWith("q="));
    if (!qParam) {
      return true;
    }

    const qValue = Number.parseFloat(qParam.trim().slice(2));
    return Number.isNaN(qValue) || qValue > 0;
  });
}

function docSlugFromPath(pathname) {
  if (!pathname.startsWith("/docs/")) {
    return null;
  }

  let slug = pathname.slice("/docs/".length);
  if (!slug) {
    return null;
  }

  if (slug.endsWith("/")) {
    slug = slug.slice(0, -1);
  }

  if (slug === "index.html") {
    return null;
  }

  if (slug.endsWith(".html")) {
    slug = slug.slice(0, -".html".length);
  }

  if (!slug || slug.includes("/") || slug.includes(".")) {
    return null;
  }

  return slug;
}

function markdownAssetPathFor(url) {
  if (url.pathname === "/" || url.pathname === "/index.html") {
    return HOME_MARKDOWN_PATH;
  }

  if (url.pathname === "/docs" || url.pathname === "/docs/" || url.pathname === "/docs/index.html") {
    return DOCS_INDEX_MARKDOWN_PATH;
  }

  const slug = docSlugFromPath(url.pathname);
  if (!slug) {
    return null;
  }

  return `${DOCS_MARKDOWN_PREFIX}/${slug}.md`;
}

function responseFromSource(request, sourceResponse, options = {}) {
  const {
    body,
    contentType,
    varyAccept = false,
  } = options;
  const headers = new Headers(sourceResponse.headers);
  headers.delete("content-length");
  if (contentType) {
    headers.set("content-type", contentType);
  }
  if (!headers.has("cache-control")) {
    headers.set("cache-control", "public, max-age=0, must-revalidate");
  }
  if (varyAccept) {
    appendVaryHeader(headers, "Accept");
  }
  appendDiscoveryHeaders(headers);
  return new Response(request.method === "HEAD" ? null : (body ?? sourceResponse.body), {
    status: sourceResponse.status,
    statusText: sourceResponse.statusText,
    headers,
  });
}

function apiCatalogDocument(origin) {
  return {
    linkset: [
      {
        anchor: `${origin}/`,
        "service-doc": [
          {
            href: `${origin}/docs/mcp`,
            type: "text/html",
            title: "Parley MCP integration",
          },
        ],
        "service-meta": [
          {
            href: `${origin}/generated/docs.json`,
            type: "application/json",
            title: "Parley docs index",
          },
        ],
      },
    ],
  };
}

function apiCatalogResponse(request) {
  const url = new URL(request.url);
  const body = JSON.stringify(apiCatalogDocument(url.origin), null, 2);
  const headers = new Headers();
  headers.set(
    "content-type",
    `application/linkset+json; profile="${API_CATALOG_PROFILE}"; charset=utf-8`
  );
  headers.set("cache-control", "public, max-age=300");
  appendDiscoveryHeaders(headers);
  return new Response(request.method === "HEAD" ? null : body, {
    status: 200,
    headers,
  });
}

async function injectDocsBootstrap(request, env, htmlPath) {
  const [htmlResponse, docsResponse] = await Promise.all([
    env.ASSETS.fetch(new Request(new URL(htmlPath, request.url), request)),
    env.ASSETS.fetch(new Request(new URL("/generated/docs.json", request.url), request)),
  ]);

  if (!htmlResponse.ok || !docsResponse.ok) {
    return htmlResponse.ok ? docsResponse : htmlResponse;
  }

  const [html, docsJson] = await Promise.all([htmlResponse.text(), docsResponse.text()]);
  const body = html.replace('"__PARLEY_DOCS_JSON__"', docsJson);
  return responseFromSource(request, htmlResponse, {
    body,
    contentType: "text/html; charset=utf-8",
    varyAccept: true,
  });
}

async function markdownResponse(request, env, markdownPath) {
  const response = await env.ASSETS.fetch(new Request(new URL(markdownPath, request.url), request));
  if (!response.ok) {
    return response;
  }

  return responseFromSource(request, response, {
    contentType: MARKDOWN_CONTENT_TYPE,
    varyAccept: true,
  });
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const wantsMarkdown = acceptsMarkdown(request);

    if (url.pathname === "/.well-known/api-catalog") {
      return apiCatalogResponse(request);
    }

    if (wantsMarkdown) {
      const markdownPath = markdownAssetPathFor(url);
      if (markdownPath) {
        return markdownResponse(request, env, markdownPath);
      }
    }

    const isDocsRoute = url.pathname === "/docs" || url.pathname === "/docs/";
    const isDocsPrettyPath =
      url.pathname.startsWith("/docs/") &&
      !url.pathname.endsWith(".html") &&
      !url.pathname.slice("/docs/".length).includes(".");

    if (isDocsRoute || isDocsPrettyPath) {
      return injectDocsBootstrap(request, env, "/docs/");
    }

    if (url.pathname === "/") {
      return injectDocsBootstrap(request, env, "/");
    }

    const response = await env.ASSETS.fetch(request);
    if (response.headers.get("content-type")?.includes("text/html")) {
      return responseFromSource(request, response, {
        contentType: response.headers.get("content-type") ?? "text/html; charset=utf-8",
        varyAccept: true,
      });
    }
    return response;
  },
};
