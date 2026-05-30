import { type CheckConfig, checkDocs, loadConfig } from "./check-docs-lib.ts";
import { deno } from "./deno-runtime.ts";

const DEFAULT_TEST_CONFIG: CheckConfig = {
  documents: {
    root: "design",
    extensions: [".md"],
  },
  links: {
    externalPrefixes: ["http://", "https://", "mailto:", "#"],
    checkDisplayedMarkdownFilename: true,
    displayedMarkdownPathPattern: "^(?:\\.\\.?/)?(?:[^/\\]]+/)*[^/\\]]+\\.md$",
  },
  numberedDocuments: {
    filenamePattern: "^(?<number>\\d{2})-.+\\.md$",
    headingPattern: "^#\\s+(?<number>\\d{2})\\b",
    referencePattern:
      "(?<![A-Za-z0-9_/.-])(?<filename>\\d{2}-[A-Za-z0-9_.-]+\\.md)(?![A-Za-z0-9_.-])",
  },
  directorySequences: [{ startAt: 0 }],
  sectionReferences: [
    {
      label: "target.md",
      targetFile: "target.md",
      lineIncludes: ["target.md", "§"],
      sectionHeadingPattern:
        "^#{2,6}\\s+(?<section>\\d+(?:\\.\\d+)*)(?:[.．])?\\s+",
      sectionRefPattern: "§\\s*(?<section>\\d+(?:\\.\\d+)*)",
    },
  ],
};

deno.test("accepts a coherent documentation tree", async () => {
  await withTempDocs(
    {
      "design/target.md": "# Ousia OS 总纲\n\n## 1. 痛点\n\n### 1.1 依赖\n",
      "design/core/00-alpha.md":
        "# 00 — Alpha\n\nSee [target.md](../target.md).\n",
      "design/topics/00-topic.md":
        "# 00 — Topic\n\nSee [00-alpha.md](../core/00-alpha.md).\n",
    },
    async (root) => {
      const result = await checkDocs(root, DEFAULT_TEST_CONFIG);
      assertEquals(
        result.errors.map((diagnostic) => diagnostic.message),
        [],
      );
    },
  );
});

deno.test("accepts a relative root path", async () => {
  await withTempDocs(
    {
      "design/target.md": "# Ousia OS 总纲\n\n## 1. 痛点\n",
      "design/core/00-alpha.md":
        "# 00 — Alpha\n\nSee [target.md](../target.md).\n",
      "design/topics/00-topic.md":
        "# 00 — Topic\n\nSee [00-alpha.md](../core/00-alpha.md).\n",
    },
    async (root) => {
      const previousCwd = deno.cwd();
      try {
        deno.chdir(root);
        const result = await checkDocs(".", DEFAULT_TEST_CONFIG);
        assertEquals(
          result.errors.map((diagnostic) => diagnostic.message),
          [],
        );
      } finally {
        deno.chdir(previousCwd);
      }
    },
  );
});

deno.test("accepts a configured documentation root", async () => {
  await withTempDocs(
    {
      "docs/index.md": "# Ousia OS 总纲\n\n## 1. 痛点\n",
      "docs/main/00-alpha.md": "# 00 — Alpha\n\nSee [index.md](../index.md).\n",
    },
    async (root) => {
      const result = await checkDocs(root, {
        ...DEFAULT_TEST_CONFIG,
        documents: { root: "docs", extensions: [".md"] },
        directorySequences: [{ startAt: 0 }],
        sectionReferences: [],
      });
      assertEquals(
        result.errors.map((diagnostic) => diagnostic.message),
        [],
      );
    },
  );
});

deno.test("loads the design project config", async () => {
  const config = await loadConfig("../../../design/check-docs.config.json");
  assertEquals(config.projectRoot, ".");
  assertEquals(config.documents.root, ".");
  assertEquals(
    config.directorySequences?.map((rule) => rule.startAt),
    [0],
  );
});

deno.test("rejects numbered H1 mismatch", async () => {
  await withTempDocs(
    {
      "design/target.md": "# Ousia OS 总纲\n\n## 1. 痛点\n",
      "design/core/00-alpha.md": "# 01 — Alpha\n",
      "design/topics/00-topic.md": "# 00 — Topic\n",
    },
    async (root) => {
      const result = await checkDocs(root, DEFAULT_TEST_CONFIG);
      assertEquals(
        result.errors.map((diagnostic) => diagnostic.message),
        ["filename/H1 number mismatch: design/core/00-alpha.md has H1 01"],
      );
    },
  );
});

deno.test("rejects broken markdown links", async () => {
  await withTempDocs(
    {
      "design/target.md": "# Ousia OS 总纲\n\n## 1. 痛点\n",
      "design/core/00-alpha.md":
        "# 00 — Alpha\n\nSee [missing.md](./missing.md).\n",
      "design/topics/00-topic.md": "# 00 — Topic\n",
    },
    async (root) => {
      const result = await checkDocs(root, DEFAULT_TEST_CONFIG);
      assertEquals(
        result.errors.map((diagnostic) => diagnostic.message),
        ["broken markdown link: design/core/00-alpha.md -> ./missing.md"],
      );
    },
  );
});

deno.test("rejects stale link display filenames", async () => {
  await withTempDocs(
    {
      "design/target.md": "# Ousia OS 总纲\n\n## 1. 痛点\n",
      "design/core/00-alpha.md":
        "# 00 — Alpha\n\nSee [09-old.md](../topics/00-topic.md).\n",
      "design/topics/00-topic.md": "# 00 — Topic\n",
    },
    async (root) => {
      const result = await checkDocs(root, DEFAULT_TEST_CONFIG);
      assertEquals(
        result.errors.map((diagnostic) => diagnostic.message),
        [
          "markdown link text does not match target filename: design/core/00-alpha.md has [09-old.md] -> 00-topic.md",
          "unknown numbered markdown filename reference in design/core/00-alpha.md: 09-old.md",
        ],
      );
    },
  );
});

deno.test("rejects unknown bare numbered markdown filenames", async () => {
  await withTempDocs(
    {
      "design/target.md": "# Ousia OS 总纲\n\n## 1. 痛点\n",
      "design/core/00-alpha.md":
        "# 00 — Alpha\n\nOld name 10-compatibility.md should fail.\n",
      "design/topics/00-topic.md": "# 00 — Topic\n",
    },
    async (root) => {
      const result = await checkDocs(root, DEFAULT_TEST_CONFIG);
      assertEquals(
        result.errors.map((diagnostic) => diagnostic.message),
        [
          "unknown numbered markdown filename reference in design/core/00-alpha.md: 10-compatibility.md",
        ],
      );
    },
  );
});

deno.test("rejects stale target section references", async () => {
  await withTempDocs(
    {
      "design/target.md": "# Ousia OS 总纲\n\n## 1. 痛点\n",
      "design/core/00-alpha.md":
        "# 00 — Alpha\n\nSee [target.md](../target.md) §4.7.\n",
      "design/topics/00-topic.md": "# 00 — Topic\n",
    },
    async (root) => {
      const result = await checkDocs(root, DEFAULT_TEST_CONFIG);
      assertEquals(
        result.errors.map((diagnostic) => diagnostic.message),
        [
          "stale target.md section reference in design/core/00-alpha.md:3: §4.7",
        ],
      );
    },
  );
});

deno.test(
  "rejects non-continuous numbered files in every directory",
  async () => {
    await withTempDocs(
      {
        "design/target.md": "# Ousia OS 总纲\n\n## 1. 痛点\n",
        "design/core/00-alpha.md": "# 00 — Alpha\n",
        "design/core/02-gamma.md": "# 02 — Gamma\n",
        "design/topics/01-topic.md": "# 01 — Topic\n",
        "design/notes/analysis/00-first.md": "# 00 — First\n",
        "design/notes/analysis/00-duplicate.md": "# 00 — Duplicate\n",
      },
      async (root) => {
        const result = await checkDocs(root, DEFAULT_TEST_CONFIG);
        assertEquals(
          result.errors.map((diagnostic) => diagnostic.message),
          [
            "numbered markdown files are not continuous in design/core: expected 00, 01, got 00, 02",
            "numbered markdown files are not continuous in design/notes/analysis: expected 00, 01, got 00, 00",
            "numbered markdown files are not continuous in design/topics: expected 00, got 01",
          ],
        );
      },
    );
  },
);

async function withTempDocs(
  files: Record<string, string>,
  run: (root: string) => Promise<void>,
): Promise<void> {
  const root = await deno.makeTempDir();
  try {
    for (const [relativePath, content] of Object.entries(files)) {
      const path = `${root}/${relativePath}`;
      await deno.mkdir(dirname(path), { recursive: true });
      await deno.writeTextFile(path, content);
    }
    await run(root);
  } finally {
    await deno.remove(root, { recursive: true });
  }
}

function dirname(path: string): string {
  const index = path.lastIndexOf("/");
  return index === -1 ? "." : path.slice(0, index);
}

function assertEquals<T>(actual: T, expected: T): void {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`Expected ${expectedJson}, got ${actualJson}`);
  }
}
