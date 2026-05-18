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

test("every overlay survives being shared on its own", async ({ page }) => {
  // Overlays travel as a string of initials, which is short and readable in an
  // address bar and correct only while the initials are distinct. Nothing in
  // the types enforces that: adding a "prediction" overlay beside "partition"
  // would give both the letter `p`, and every shared link would quietly turn on
  // an overlay its author never chose. Round-tripping each one alone is what
  // makes that a failure instead of a surprise.
  //
  // The labels are read from the page rather than listed here, so an overlay
  // added to the interface is covered without this file being told about it.
  await page.goto("/");
  const toggles = page.locator("ul.toggles input[type=checkbox]");
  // allInnerTexts() reads whatever has rendered so far and never waits, so wait for the toggles.
  await expect(toggles.first()).toBeVisible();
  const labels = await page.locator("ul.toggles label").allInnerTexts();
  expect(labels.length).toBeGreaterThan(1);

  for (const label of labels) {
    const wanted = label.trim();
    await page.goto("/");
    for (const other of labels) {
      const box = page.getByLabel(other.trim());
      if (other.trim() === wanted) await box.check();
      else await box.uncheck();
    }

    const shared = page.url();
    expect(shared).toContain("#");

    // A blank page in between, because navigating to a URL that differs only
    // in its fragment is a same-document navigation: the page would keep the
    // state this loop just set and the assertions below would be checking the
    // checkboxes against themselves.
    await page.goto("about:blank");
    await page.goto(shared);

    // Exactly one box comes back checked, and it is the one that was shared.
    await expect(page.getByLabel(wanted)).toBeChecked();
    const checked = await toggles.evaluateAll((boxes) =>
      boxes.filter((box) => (box as HTMLInputElement).checked).length,
    );
    expect(checked, `sharing "${wanted}" restored ${checked} overlays`).toBe(1);
  }
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

test("the rate–distortion tab draws real curves and names its ablations", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Rate and distortion" }).click();

  // The caveat is not decoration. A page that showed rate-distortion curves
  // without it would be inviting exactly the comparison the repository refuses
  // to make, so its absence is a failure.
  await expect(page.getByText(/Orientation, not a race/)).toBeVisible();

  // A canvas with nothing drawn on it is the failure mode a screenshot would
  // miss, so the pixels are checked rather than the element.
  const drawn = await page.evaluate(() => {
    const canvas = document.querySelector<HTMLCanvasElement>(".chart canvas");
    const context = canvas?.getContext("2d");
    if (!canvas || !context) return false;
    const { data } = context.getImageData(0, 0, canvas.width, canvas.height);
    for (let index = 3; index < data.length; index += 4) {
      if (data[index] !== 0) return true;
    }
    return false;
  });
  expect(drawn).toBe(true);

  // Every ablation has to reach the table with a figure or a named refusal,
  // never a blank cell.
  const table = page.locator("table.ablations").first();
  for (const toolset of ["no-golden", "no-skip", "no-inter", "no-subpel", "no-split"]) {
    await expect(table.getByRole("rowheader", { name: toolset })).toBeVisible();
  }
  const figures = await table.locator("tbody td:first-of-type").allInnerTexts();
  expect(figures).toHaveLength(5);
  for (const figure of figures) expect(figure.trim()).not.toBe("");

  // And the figures are the receipt's, not the page's. The check above would
  // pass for a page that recomputed the bitrate difference from the points and
  // quietly disagreed with the receipt it draws, which is the whole failure the
  // arithmetic was removed from this bundle to prevent. So the receipt the page
  // itself loaded is fetched and every rendered cell is matched against it.
  const expected = await page.evaluate(async () => {
    const receipt = await (await fetch("./rd-campaign.json")).json();
    const first = receipt.curves[0].clip;
    return receipt.curves
      .filter((curve: any) => curve.clip === first && curve.toolset !== "full")
      .map((curve: any) =>
        typeof curve.bd_rate_percent === "number"
          ? `${curve.bd_rate_percent >= 0 ? "+" : ""}${curve.bd_rate_percent.toFixed(2)}%`
          : String(curve.bd_rate_refused),
      );
  });
  expect(expected).toHaveLength(5);
  expect(figures.map((figure) => figure.trim())).toEqual(expected);

  // Switching the metric has to redraw rather than leave the previous chart up.
  // The radio is addressed by role because the charts carry the metric in their
  // own accessible names, and a label lookup would match three things.
  const ssim = page.getByRole("radio", { name: "SSIM-Y" });
  await ssim.check();
  await expect(ssim).toBeChecked();
  await expect(
    page.getByRole("img", { name: /Rate against SSIM-Y/ }).first(),
  ).toBeVisible();

  // The tab travels in a shared link like every other piece of view state.
  await expect(page).toHaveURL(/#.*t=curves/);

  // Narrowing the window has to redraw the plot at the new width. A canvas that
  // kept its old backing store would show a stretched copy of the chart, which
  // is the failure mode a fixed-size screenshot never catches.
  const widthOf = () =>
    page.evaluate(() => {
      const canvas = document.querySelector<HTMLCanvasElement>(".chart canvas");
      return canvas ? { backing: canvas.width, laid: Math.round(canvas.getBoundingClientRect().width) } : null;
    });
  const before = await widthOf();
  const ratio = await page.evaluate(() => window.devicePixelRatio || 1);
  await page.setViewportSize({ width: 720, height: 900 });

  // Polled as one condition rather than two steps. The layout width changes as
  // soon as the viewport does, but the backing store is only resized when the
  // observer fires, so checking "the width changed" and then "the backing
  // matches" reads the second one during the gap between them and fails on a
  // canvas that was about to be correct.
  await expect
    .poll(async () => {
      const size = await widthOf();
      if (!size || size.laid === before?.laid) return "not resized yet";
      return size.backing === Math.round(size.laid * ratio)
        ? "redrawn"
        : `backing ${size.backing} against ${size.laid} laid out at ratio ${ratio}`;
    })
    .toBe("redrawn");
});

test("the ugly-block action refuses to answer without a source clip", async ({ page }) => {
  await page.goto("/");
  // The page can show what the decoder produced. It cannot show how wrong that
  // is without the original, and the plan for this feature is explicit that it
  // must say so rather than estimate.
  const button = page.getByRole("button", { name: "Why is this block ugly?" });
  await expect(button).toBeDisabled();
  await expect(button).toHaveAttribute("title", /cannot measure error, and it will not guess/);
});

test("with the source loaded, the ugly-block action names a block and its error", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByText("64×64")).toBeVisible();

  // The source is built in the page rather than shipped. The size budget
  // refuses any .y4m in the build for good reason — no decoded video may reach
  // the page — so the test synthesises one instead of weakening that rule.
  await attachSource(page, 64, 64, 3, (at, index) => (at * 7 + index * 31) % 256);
  await expect(page.getByText("Source loaded")).toBeVisible();

  const button = page.getByRole("button", { name: "Why is this block ugly?" });
  await expect(button).toBeEnabled();
  await button.click();

  // A block is selected and the panel explains why that one, with a measured
  // error rather than a score.
  const panel = page.locator(".side .card").last();
  await expect(panel.getByText("Why this one")).toBeVisible();
  await expect(panel.getByText(/per sample$/)).toBeVisible();
  await expect(panel.getByText(/most error for the fewest bits/)).toBeVisible();
  await expect(page).toHaveURL(/#.*b=\d+,\d+/);
});

test("a source clip that does not match the stream is refused", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("64×64")).toBeVisible();

  // Wrong dimensions. Accepting it would produce an error map computed against
  // the wrong pictures, which looks plausible and means nothing.
  await attachSource(page, 32, 32, 1, () => 64);
  await expect(page.getByText(/the source is 32x32 and the stream is 64x64/)).toBeVisible();
  await expect(page.getByRole("button", { name: "Why is this block ugly?" })).toBeDisabled();
});

/// Hands the page a synthetic Y4M through its own file input.
///
/// The clip is assembled inside the browser and delivered with a `DataTransfer`
/// rather than through Playwright's file API, which would need Node's `Buffer`
/// and therefore a types-only dependency this repository does not want. The
/// page sees an ordinary file-picker change either way.
async function attachSource(
  page: import("@playwright/test").Page,
  width: number,
  height: number,
  frames: number,
  sample: (at: number, frame: number) => number,
): Promise<void> {
  await page.evaluate(
    ({ width, height, frames, body }) => {
      const value = new Function("at", "frame", `return (${body})(at, frame);`) as (
        at: number,
        frame: number,
      ) => number;
      const encoder = new TextEncoder();
      const header = encoder.encode(`YUV4MPEG2 W${width} H${height} F24:1 Ip A1:1 C420jpeg\n`);
      const marker = encoder.encode("FRAME\n");
      const luma = width * height;
      const chroma = Math.ceil(width / 2) * Math.ceil(height / 2);
      const bytes = new Uint8Array(header.length + frames * (marker.length + luma + 2 * chroma));
      bytes.set(header, 0);
      let at = header.length;
      for (let frame = 0; frame < frames; frame += 1) {
        bytes.set(marker, at);
        at += marker.length;
        for (let index = 0; index < luma; index += 1) bytes[at + index] = value(index, frame) & 0xff;
        at += luma;
        bytes.fill(128, at, at + 2 * chroma);
        at += 2 * chroma;
      }

      const input = document.querySelector<HTMLInputElement>('input[type="file"][accept=".y4m"]');
      if (!input) throw new Error("the source input is not on the page");
      const transfer = new DataTransfer();
      transfer.items.add(new File([bytes], "source.y4m", { type: "application/octet-stream" }));
      input.files = transfer.files;
      input.dispatchEvent(new Event("change", { bubbles: true }));
    },
    { width, height, frames, body: sample.toString() },
  );
}

test("average-bitrate accuracy is reported apart from the curves", async ({ page }) => {
  await page.goto("/#t=curves");

  // The table has to exist and carry every target the receipt measured.
  const table = page.locator("table.ablations").last();
  await expect(table.locator("caption")).toContainText(/rate controller delivered/);
  const rows = table.locator("tbody tr");
  await expect(rows).toHaveCount(6);

  // And it must not be plotted. Three average-bitrate points are not a curve —
  // the quality at each is an outcome, not a setting — so the charts must carry
  // only the toolsets, never a seventh series made of these.
  const series = await page.evaluate(() =>
    [...document.querySelectorAll<HTMLCanvasElement>(".chart canvas")].map(
      (canvas) => canvas.getAttribute("aria-label") ?? "",
    ),
  );
  expect(series.length).toBeGreaterThan(0);
  for (const label of series) {
    expect(label).toMatch(/for 6 toolsets$/);
  }
});
