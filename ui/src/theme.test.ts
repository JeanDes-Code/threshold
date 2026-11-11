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

  it("the NPC colours have hues at least 30 degrees apart", () => {
    const tokens = css["./tokens.css"] ?? "";
    const hues = [...tokens.matchAll(/--npc-\d+:\s*#([0-9a-f]{6})/gi)].map((m) => hue(m[1] ?? ""));
    expect(hues).toHaveLength(8);
    for (const [i, a] of hues.entries()) {
      for (const b of hues.slice(i + 1)) {
        const apart = Math.min(Math.abs(a - b), 360 - Math.abs(a - b));
        expect(apart).toBeGreaterThanOrEqual(30);
      }
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

/** The hue of a hex colour "rrggbb", in degrees. */
function hue(hex: string): number {
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255) as [number, number, number];
  const max = Math.max(r, g, b);
  const d = max - Math.min(r, g, b);
  if (d === 0) return 0;
  const h = max === r ? ((g - b) / d) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return (h * 60 + 360) % 360;
}
