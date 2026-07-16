import "@testing-library/jest-dom/vitest";
import { afterEach, describe, expect, it, vi } from "vitest";

import { buildStandaloneHtml } from "./htmlExport";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("standalone HTML export security", () => {
  it("adds a no-network CSP and strips adversarial URL, event and CSS attributes", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(new Uint8Array([137, 80, 78, 71]), {
      status: 200,
      headers: { "content-type": "image/png" },
    })));
    const preview = document.createElement("section");
    preview.innerHTML = [
      '<a data-export-href="https://example.com/safe" ping="https://evil.example/ping" onclick="bad()" style="background:url(https://evil.example/css)">safe</a>',
      '<img alt="local" src="asset://localhost/local.png" srcset="https://evil.example/a.png 2x" lowsrc="https://evil.example/low.png" dynsrc="https://evil.example/dynamic.png" onerror="bad()" style="background-image:url(https://evil.example/b.png)">',
      '<video poster="https://evil.example/poster.png"><source src="https://evil.example/movie.mp4"></video>',
      '<form action="https://evil.example/post"><button formaction="https://evil.example/button">send</button></form>',
      '<svg><use href="https://evil.example/sprite.svg#icon" xlink:href="https://evil.example/x.svg#icon"></use></svg>',
      '<object data="https://evil.example/object"></object>',
      '<applet archive="https://evil.example/archive.jar" codebase="https://evil.example/"></applet>',
    ].join("");

    const html = await buildStandaloneHtml(preview, "Safe export");
    const exported = new DOMParser().parseFromString(html, "text/html");
    const csp = exported.querySelector('meta[http-equiv="Content-Security-Policy"]');
    expect(csp?.getAttribute("content")).toBe(
      "default-src 'none'; base-uri 'none'; form-action 'none'; connect-src 'none'; object-src 'none'; frame-src 'none'; media-src 'none'; img-src data:; style-src 'unsafe-inline'",
    );
    expect(exported.querySelector("a")?.getAttribute("href")).toBe("https://example.com/safe");
    expect(exported.querySelector("img")?.getAttribute("src")).toBe("data:image/png;base64,iVBORw==");
    for (const attribute of ["ping", "srcset", "lowsrc", "dynsrc", "poster", "action", "formaction", "xlink:href", "data", "archive", "codebase", "style"]) {
      expect(exported.querySelector(`[${attribute.replace(":", "\\:")}]`)).toBeNull();
    }
    expect(html).not.toMatch(/evil\.example|\son[a-z]+\s*=/i);
    expect(exported.querySelector("video, source, form, object, applet, use")).toBeNull();
  });

  it("streams local images and cancels as soon as an undeclared body exceeds the per-image limit", async () => {
    const cancel = vi.fn();
    const body = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new Uint8Array(4 * 1024 * 1024));
        controller.enqueue(new Uint8Array(2 * 1024 * 1024));
      },
      cancel,
    });
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(body, {
      status: 200,
      headers: { "content-type": "image/png" },
    })));
    const preview = document.createElement("section");
    preview.innerHTML = '<img alt="huge" src="asset://localhost/huge.png">';

    await expect(buildStandaloneHtml(preview, "Huge")).rejects.toThrow("export image is too large");
    expect(cancel).toHaveBeenCalledTimes(1);
  });
});
