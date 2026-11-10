import { describe, expect, it } from "vitest";

const css = import.meta.glob<string>("./**/*.css", { query: "?raw", import: "default", eager: true });
const fontFiles = Object.keys(import.meta.glob("./assets/fonts/*.woff2"));
const licences = Object.keys(import.meta.glob("./assets/fonts/OFL-*.txt"));

describe("theme", () => {
  it("no colour outside the tokens file", () => {
    expect(Object.keys(css)).toContain("./tokens.css");
    for (const [path, text] of Object.entries(css)) {
      // Vitest gives empty CSS unless `test.css` includes it: never pass on nothing.
      expect(text.length, path).toBeGreaterThan(0);
      if (path === "./tokens.css") continue;
      expect(text, path).not.toMatch(/#[0-9a-f]{3,8}\b/i);
      expect(text, path).not.toMatch(/rgba?\(|hsla?\(/i);
    }
  });

  it("fonts ship with the app", () => {
    for (const [path, text] of Object.entries(css)) expect(text, path).not.toMatch(/http/i);

    const fonts = css["./fonts.css"];
    expect(fonts).toBeDefined();
    const urls = [...(fonts ?? "").matchAll(/url\(\s*["']?([^"')]+)["']?\s*\)/g)].map((m) => m[1]);
    expect(urls.length).toBeGreaterThan(0);
    for (const url of urls) expect(fontFiles).toContain(url);

    // One licence per family: the family is the file name before "-roman" or "-italic".
    const families = new Set(fontFiles.map((f) => f.replace(/^.*\//, "").replace(/-(roman|italic)\.woff2$/, "")));
    expect(families.size).toBeGreaterThanOrEqual(2);
    for (const family of families) expect(licences).toContain(`./assets/fonts/OFL-${family}.txt`);
  });
});
