import assert from "node:assert/strict";
import test from "node:test";

import {
  isSameResearchPaper,
  normalizeResearchDoi,
  normalizeResearchTitle,
  researchPaperKey,
} from "./researchKnowledgeBaseCore.ts";

test("normalizes DOI prefixes and casing", () => {
  assert.equal(normalizeResearchDoi("https://doi.org/10.1016/J.CIRP.2024.01.001"), "10.1016/j.cirp.2024.01.001");
  assert.equal(normalizeResearchDoi("doi: 10.1000/ABC."), "10.1000/abc");
  assert.equal(normalizeResearchDoi("  "), null);
});

test("deduplicates papers by normalized DOI", () => {
  const paper = {
    title: "A Digital Twin Method for Machining",
    publicationYear: 2024,
    doi: "https://doi.org/10.1000/XYZ",
  };
  const note = {
    title: "论文｜另一标题",
    content: "- DOI：10.1000/xyz\n- 发表时间：2024",
  };
  assert.equal(isSameResearchPaper(note, paper), true);
  assert.equal(researchPaperKey(paper), "doi:10.1000/xyz");
});

test("deduplicates papers without DOI by normalized title and year", () => {
  const paper = {
    title: "AI-Driven Process Planning: A Review",
    publicationYear: 2023,
    doi: null,
  };
  const note = {
    title: "论文｜AI Driven Process Planning — A Review",
    content: "- 发表时间：2023\n- DOI：未收录",
  };
  assert.equal(normalizeResearchTitle(paper.title), "ai driven process planning a review");
  assert.equal(isSameResearchPaper(note, paper), true);
});
