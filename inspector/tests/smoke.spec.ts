import { expect, test } from "@playwright/test";

// What the site has to do before anyone should be shown it: load a clip,
// decode it in the browser, scrub, draw overlays, answer a click with real
// syntax, and list the bug catalogue with a stream behind each entry.
//
// Every assertion here is about something the decoder produced. A test that
// only checked the page rendered would pass on a page that rendered nothing
// but its own furniture.

test("the page loads without an error in the console", async ({ page }) => {
  // A page that renders and throws is a page that will lose a feature the
  // moment someone touches it, so the console is part of the contract.
  const failures: string[] = [];
  page.on("pageerror", (error) => failures.push(`uncaught: ${error.message}`));
  page.on("console", (message) => {
    if (message.type() === "error") failures.push(message.text());
  });
  await page.goto("/");
  await expect(page.getByText(/input bytes = canonical replay/)).toBeVisible();
  expect(failures).toEqual([]);
});

test("the sample clip loads and decodes in the browser", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Projection room" })).toBeVisible();

  // The stream facts come from the module, so their presence proves the
  // WebAssembly boundary answered.
  await expect(page.getByText("64×64")).toBeVisible();
  await expect(page.getByText("3 at 24/1 fps")).toBeVisible();
  await expect(page.getByText(/input bytes = canonical replay/)).toBeVisible();

  // The picture canvas must carry decoded samples, not an empty buffer.
  const nonBlank = await page.evaluate(() => {
    const canvas = document.querySelector<HTMLCanvasElement>("canvas.picture");
    if (!canvas) return false;
    const context = canvas.getContext("2d");
    if (!context) return false;
    const { data } = context.getImageData(0, 0, canvas.width, canvas.height);
    for (let index = 0; index < data.length; index += 4) {
      if (data[index] !== 0 || data[index + 1] !== 0 || data[index + 2] !== 0) return true;
    }
    return false;
  });
  expect(nonBlank).toBe(true);
});

test("scrubbing decodes a later frame", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("1 decode from keyframe 0")).toBeVisible();

  const slider = page.getByRole("slider");
  await slider.fill("2");

  // Frame two of the sample sits two frames behind its keyframe, so a correct
  // seek reports three decodes rather than one.
  await expect(page.getByText("3 decodes from keyframe 0")).toBeVisible();
  await expect(page).toHaveURL(/#.*f=2/);
});

test("overlays draw on top of the picture", async ({ page }) => {
  await page.goto("/");
  const overlayIsBlank = async () =>
    page.evaluate(() => {
      const canvas = document.querySelector<HTMLCanvasElement>("canvas.overlay");
      const context = canvas?.getContext("2d");
      if (!canvas || !context) return true;
      const { data } = context.getImageData(0, 0, canvas.width, canvas.height);
      for (let index = 3; index < data.length; index += 4) {
        if (data[index] !== 0) return false;
      }
      return true;
    });

  // The partition grid is on by default, so something must already be drawn.
  await expect.poll(overlayIsBlank).toBe(false);

  await page.getByLabel("Partition grid").uncheck();
  await page.getByLabel("Motion vectors").uncheck();
  await page.getByLabel("Intra glyphs").uncheck();
  await expect.poll(overlayIsBlank).toBe(true);

  await page.getByLabel("Entropy heatmap").check();
  await expect.poll(overlayIsBlank).toBe(false);
  await expect(page).toHaveURL(/#.*o=e/);
});

test("clicking a block shows its syntax", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Click the picture to select a coding block.")).toBeVisible();

  const overlay = page.locator("canvas.overlay");
  const box = await overlay.boundingBox();
  expect(box).not.toBeNull();
  if (!box) return;
  await page.mouse.click(box.x + box.width * 0.3, box.y + box.height * 0.3);

  const panel = page.locator(".side .card").last();
  // Two positions are shown: the block's and its superblock's.
  await expect(panel.getByText("Position").first()).toBeVisible();
  await expect(panel.getByText(/intra|inter|skip/)).toBeVisible();
  // Both accounting figures are shown in bits: the block's and its superblock's.
  await expect(panel.getByText(/bits$/).first()).toBeVisible();
  // The page must never call either accounting figure the block's bit count.
  await expect(panel.getByText(/That question has no answer/)).toBeVisible();
  await expect(page).toHaveURL(/#.*b=\d+,\d+/);
});

test("a shared link restores the view it encoded", async ({ page }) => {
  await page.goto("/#f=1&o=pe&h=emitted&b=0,0");
  await expect(page.getByText("2 decodes from keyframe 0")).toBeVisible();
  await expect(page.getByLabel("Partition grid")).toBeChecked();
  await expect(page.getByLabel("Entropy heatmap")).toBeChecked();
  await expect(page.getByLabel("Motion vectors")).not.toBeChecked();
  await expect(page.getByLabel("Emission-time bytes")).toBeChecked();
  await expect(page.locator(".side .card").last().getByText("Position").first()).toBeVisible();
});

test("the cutting room lists findings and opens their streams", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Cutting room" }).click();

  const finding = page.locator(".finding").first();
  await expect(finding).toBeVisible();
  await expect(finding.getByText(/conformance\/crashes\//)).toBeVisible();

  await finding.getByRole("button", { name: /Open this regression stream/ }).click();

  // The regression stream opens in the projection room and decodes there: an
  // entry that could only be read about would be a claim, not a reproduction.
  await expect(page.getByRole("heading", { name: "Projection room" })).toBeVisible();
  await expect(page.getByText(/^regression /)).toBeVisible();
  await expect(page.getByRole("slider")).toBeVisible();
});
