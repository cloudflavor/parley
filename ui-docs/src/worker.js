export default {
  async fetch(request, env) {
    const url = new URL(request.url);

    const isDocsRoute = url.pathname === "/docs" || url.pathname === "/docs/";
    const isDocsPrettyPath =
      url.pathname.startsWith("/docs/") &&
      !url.pathname.endsWith(".html") &&
      !url.pathname.slice("/docs/".length).includes(".");

    if (isDocsRoute || isDocsPrettyPath) {
      const rewrite = new URL("/docs/index.html", url);
      return env.ASSETS.fetch(new Request(rewrite, request));
    }

    if (url.pathname === "/") {
      const rewrite = new URL("/index.html", url);
      return env.ASSETS.fetch(new Request(rewrite, request));
    }

    return env.ASSETS.fetch(request);
  },
};
