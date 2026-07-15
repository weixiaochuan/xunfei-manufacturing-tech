import { create } from "zustand";
import type {
  PptChunkUnderstandingDraft,
  PptMasterGenerateResult,
  PptMaterialAnalysisProgress,
  PptMaterialAnalysisStatus,
  PptMaterialChunkPlan,
  PptMaterialProcessingMode,
  PptUnderstandingDraft,
  ResolvedPptMaterialSource,
} from "@/types";

export type PptMaterialInputMode = "manual" | "internal";
export type PptGenerationMode = "agent" | "template";
export type PptDraftRequestStatus = "idle" | "loading" | "success" | "error";

export interface PptSmartDraftFields {
  topic: string;
  audience: string;
  customAudience?: string;
  pageCount: string;
  customPageCount?: number;
  style: string;
  customStyle?: string;
  extraRequirements?: string;
}

interface PptGenerationDraftState {
  activeStep: number;
  activeMode: "smart" | "advanced";
  smartFields: PptSmartDraftFields;
  selectedModelId: number | null;
  selectedModelInitialized: boolean;
  generationMode: PptGenerationMode;
  outputDir: string;
  outputDirInitialized: boolean;

  materialInputMode: PptMaterialInputMode;
  manualRawMaterial: string;
  resolvedMaterialSources: ResolvedPptMaterialSource[];
  mergedMaterialText: string;
  mergedMaterialEdited: boolean;
  materialRevision: number;
  understandingRevision: number | null;
  materialUnderstandingStale: boolean;
  materialProcessingMode: PptMaterialProcessingMode;
  materialChunkPlan: PptMaterialChunkPlan | null;
  chunkUnderstandingDrafts: PptChunkUnderstandingDraft[];
  failedChunkIndexes: number[];
  materialAnalysisStatus: PptMaterialAnalysisStatus;
  materialAnalysisProgress: PptMaterialAnalysisProgress | null;
  materialAnalysisError: string | null;
  chunkAnalysisRevision: number | null;
  materialAnalysisRunId: number;

  understandingDraft: PptUnderstandingDraft | null;
  understandingDraftDirty: boolean;
  understandingStatus: PptDraftRequestStatus;
  understandingError: string | null;
  generationStatus: PptDraftRequestStatus;
  generationError: string | null;
  generationResult: PptMasterGenerateResult | null;
  updatedAt: number | null;

  setActiveMode: (mode: "smart" | "advanced") => void;
  setBasicFields: (fields: Partial<PptSmartDraftFields>) => void;
  setSelectedModelId: (modelId: number | null) => void;
  initializeSelectedModel: (modelId: number | null) => void;
  setGenerationMode: (mode: PptGenerationMode) => void;
  setOutputDir: (path: string) => void;
  initializeOutputDir: (path: string) => void;
  setMaterialInputMode: (mode: PptMaterialInputMode) => void;
  setManualRawMaterial: (text: string) => void;
  replaceInternalMaterial: (
    sources: ResolvedPptMaterialSource[],
    mergedText: string,
    edited: boolean,
  ) => void;
  setInternalSourcesOnly: (sources: ResolvedPptMaterialSource[]) => void;
  setMergedMaterialText: (text: string, edited: boolean) => void;
  clearInternalMaterial: () => void;
  beginMaterialAnalysis: (
    mode: Exclude<PptMaterialProcessingMode, null>,
    materialRevision: number,
    preserveArtifacts?: boolean,
  ) => number;
  setMaterialChunkPlan: (
    plan: PptMaterialChunkPlan,
    materialRevision: number,
    runId: number,
  ) => void;
  setMaterialAnalysisStage: (
    status: PptMaterialAnalysisStatus,
    progress: PptMaterialAnalysisProgress,
    materialRevision: number,
    runId: number,
  ) => void;
  cacheChunkUnderstandingDraft: (
    draft: PptChunkUnderstandingDraft,
    materialRevision: number,
    runId: number,
  ) => void;
  setMaterialAnalysisError: (
    error: string,
    failedChunkIndexes: number[],
    materialRevision: number,
    runId: number,
  ) => void;
  finishMaterialAnalysis: (materialRevision: number, runId: number) => void;
  cancelMaterialAnalysis: () => void;
  setUnderstandingDraft: (draft: PptUnderstandingDraft, materialRevision: number) => void;
  updateUnderstandingField: (field: keyof PptUnderstandingDraft, value: string) => void;
  setUnderstandingStatus: (status: PptDraftRequestStatus, error?: string | null) => void;
  setGenerationStatus: (status: PptDraftRequestStatus, error?: string | null) => void;
  setGenerationResult: (result: PptMasterGenerateResult | null) => void;
  setGenerationError: (error: string | null) => void;
  resetPptDraft: () => void;
}

const defaultSmartFields: PptSmartDraftFields = {
  topic: "",
  audience: "老师/评委",
  pageCount: "6 页",
  style: "科技蓝",
  extraRequirements: "",
};

function effectiveMaterial(state: Pick<
  PptGenerationDraftState,
  "materialInputMode" | "manualRawMaterial" | "mergedMaterialText"
>): string {
  return state.materialInputMode === "internal"
    ? state.mergedMaterialText
    : state.manualRawMaterial;
}

export function getEffectivePptRawMaterial(state: Pick<
  PptGenerationDraftState,
  "materialInputMode" | "manualRawMaterial" | "mergedMaterialText"
>): string {
  return effectiveMaterial(state);
}

function materialChangedPatch(state: PptGenerationDraftState) {
  const hadUnderstanding = Boolean(state.understandingDraft) || state.understandingRevision !== null;
  return {
    materialRevision: state.materialRevision + 1,
    understandingRevision: null,
    understandingDraft: null,
    understandingDraftDirty: false,
    understandingStatus: "idle" as const,
    understandingError: null,
    materialUnderstandingStale: state.materialUnderstandingStale || hadUnderstanding,
    materialProcessingMode: null,
    materialChunkPlan: null,
    chunkUnderstandingDrafts: [],
    failedChunkIndexes: [],
    materialAnalysisStatus: "idle" as const,
    materialAnalysisProgress: null,
    materialAnalysisError: null,
    chunkAnalysisRevision: null,
    materialAnalysisRunId: state.materialAnalysisRunId + 1,
    activeStep: 0,
    generationError: null,
    updatedAt: Date.now(),
  };
}

export const usePptGenerationDraftStore = create<PptGenerationDraftState>((set, get) => ({
  activeStep: 0,
  activeMode: "smart",
  smartFields: { ...defaultSmartFields },
  selectedModelId: null,
  selectedModelInitialized: false,
  generationMode: "template",
  outputDir: "",
  outputDirInitialized: false,

  materialInputMode: "manual",
  manualRawMaterial: "",
  resolvedMaterialSources: [],
  mergedMaterialText: "",
  mergedMaterialEdited: false,
  materialRevision: 0,
  understandingRevision: null,
  materialUnderstandingStale: false,
  materialProcessingMode: null,
  materialChunkPlan: null,
  chunkUnderstandingDrafts: [],
  failedChunkIndexes: [],
  materialAnalysisStatus: "idle",
  materialAnalysisProgress: null,
  materialAnalysisError: null,
  chunkAnalysisRevision: null,
  materialAnalysisRunId: 0,

  understandingDraft: null,
  understandingDraftDirty: false,
  understandingStatus: "idle",
  understandingError: null,
  generationStatus: "idle",
  generationError: null,
  generationResult: null,
  updatedAt: null,

  setActiveMode: (activeMode) => set({ activeMode, updatedAt: Date.now() }),
  setBasicFields: (fields) =>
    set((state) => ({
      smartFields: { ...state.smartFields, ...fields },
      updatedAt: Date.now(),
    })),
  setSelectedModelId: (selectedModelId) =>
    set({ selectedModelId, selectedModelInitialized: true, updatedAt: Date.now() }),
  initializeSelectedModel: (selectedModelId) => {
    if (get().selectedModelInitialized) return;
    set({ selectedModelId, selectedModelInitialized: true });
  },
  setGenerationMode: (generationMode) => set({ generationMode, updatedAt: Date.now() }),
  setOutputDir: (outputDir) =>
    set({ outputDir, outputDirInitialized: true, updatedAt: Date.now() }),
  initializeOutputDir: (outputDir) => {
    if (get().outputDirInitialized) return;
    set({ outputDir, outputDirInitialized: true });
  },
  setMaterialInputMode: (materialInputMode) =>
    set((state) => {
      if (state.materialInputMode === materialInputMode) return state;
      const before = effectiveMaterial(state);
      const after = materialInputMode === "internal"
        ? state.mergedMaterialText
        : state.manualRawMaterial;
      return {
        materialInputMode,
        ...(before === after ? { updatedAt: Date.now() } : materialChangedPatch(state)),
      };
    }),
  setManualRawMaterial: (manualRawMaterial) =>
    set((state) => {
      if (state.manualRawMaterial === manualRawMaterial) return state;
      return { manualRawMaterial, ...materialChangedPatch(state) };
    }),
  replaceInternalMaterial: (resolvedMaterialSources, mergedMaterialText, mergedMaterialEdited) =>
    set((state) => {
      const sourcesUnchanged =
        state.resolvedMaterialSources.length === resolvedMaterialSources.length &&
        state.resolvedMaterialSources.every((source, index) => {
          const next = resolvedMaterialSources[index];
          return Boolean(
            next &&
              source.id === next.id &&
              source.title === next.title &&
              source.plainText === next.plainText,
          );
        });
      if (sourcesUnchanged && state.mergedMaterialText === mergedMaterialText && state.mergedMaterialEdited === mergedMaterialEdited) {
        return state;
      }
      return {
        resolvedMaterialSources,
        mergedMaterialText,
        mergedMaterialEdited,
        ...materialChangedPatch(state),
      };
    }),
  setInternalSourcesOnly: (resolvedMaterialSources) =>
    set((state) => ({ resolvedMaterialSources, ...materialChangedPatch(state) })),
  setMergedMaterialText: (mergedMaterialText, mergedMaterialEdited) =>
    set((state) => {
      if (state.mergedMaterialText === mergedMaterialText && state.mergedMaterialEdited === mergedMaterialEdited) {
        return state;
      }
      return { mergedMaterialText, mergedMaterialEdited, ...materialChangedPatch(state) };
    }),
  clearInternalMaterial: () =>
    set((state) => {
      if (state.resolvedMaterialSources.length === 0 && !state.mergedMaterialText) return state;
      return {
        resolvedMaterialSources: [],
        mergedMaterialText: "",
        mergedMaterialEdited: false,
        ...materialChangedPatch(state),
      };
    }),
  beginMaterialAnalysis: (materialProcessingMode, materialAnalysisRevision, preserveArtifacts = false) => {
    let nextRunId = get().materialAnalysisRunId;
    set((state) => {
      if (state.materialRevision !== materialAnalysisRevision) return state;
      nextRunId = state.materialAnalysisRunId + 1;
      const canPreserve = preserveArtifacts && state.chunkAnalysisRevision === materialAnalysisRevision;
      const materialAnalysisStatus = materialProcessingMode === "direct" ? "analyzing" : "planning";
      return {
        materialProcessingMode,
        materialAnalysisStatus,
        materialAnalysisProgress: {
          current: 0,
          total: materialProcessingMode === "direct"
            ? 1
            : canPreserve
              ? state.materialChunkPlan?.chunks.length ?? 0
              : 0,
          stage: materialProcessingMode === "direct" ? "analyzing" : "planning",
        },
        materialAnalysisError: null,
        failedChunkIndexes: [],
        chunkAnalysisRevision: materialAnalysisRevision,
        materialAnalysisRunId: nextRunId,
        ...(canPreserve
          ? {}
          : {
              materialChunkPlan: null,
              chunkUnderstandingDrafts: [],
            }),
        updatedAt: Date.now(),
      };
    });
    return nextRunId;
  },
  setMaterialChunkPlan: (materialChunkPlan, materialAnalysisRevision, runId) =>
    set((state) => {
      if (
        state.materialRevision !== materialAnalysisRevision ||
        state.materialAnalysisRunId !== runId
      ) return state;
      return {
        materialChunkPlan,
        chunkAnalysisRevision: materialAnalysisRevision,
        materialAnalysisStatus: "planning",
        materialAnalysisProgress: {
          current: 0,
          total: materialChunkPlan.chunks.length,
          stage: "planning",
        },
        updatedAt: Date.now(),
      };
    }),
  setMaterialAnalysisStage: (materialAnalysisStatus, materialAnalysisProgress, materialAnalysisRevision, runId) =>
    set((state) => {
      if (
        state.materialRevision !== materialAnalysisRevision ||
        state.materialAnalysisRunId !== runId
      ) return state;
      return {
        materialAnalysisStatus,
        materialAnalysisProgress,
        updatedAt: Date.now(),
      };
    }),
  cacheChunkUnderstandingDraft: (draft, materialAnalysisRevision, runId) =>
    set((state) => {
      if (
        state.materialRevision !== materialAnalysisRevision ||
        state.materialAnalysisRunId !== runId
      ) return state;
      const chunkUnderstandingDrafts = state.chunkUnderstandingDrafts
        .filter((item) => item.chunkId !== draft.chunkId)
        .concat(draft)
        .sort((left, right) => left.chunkIndex - right.chunkIndex);
      return { chunkUnderstandingDrafts, updatedAt: Date.now() };
    }),
  setMaterialAnalysisError: (materialAnalysisError, failedChunkIndexes, materialAnalysisRevision, runId) =>
    set((state) => {
      if (
        state.materialRevision !== materialAnalysisRevision ||
        state.materialAnalysisRunId !== runId
      ) return state;
      return {
        materialAnalysisStatus: "error",
        materialAnalysisProgress: null,
        materialAnalysisError,
        failedChunkIndexes: [...failedChunkIndexes].sort((left, right) => left - right),
        updatedAt: Date.now(),
      };
    }),
  finishMaterialAnalysis: (materialAnalysisRevision, runId) =>
    set((state) => {
      if (
        state.materialRevision !== materialAnalysisRevision ||
        state.materialAnalysisRunId !== runId
      ) return state;
      return {
        materialAnalysisStatus: "success",
        materialAnalysisProgress: null,
        materialAnalysisError: null,
        failedChunkIndexes: [],
        updatedAt: Date.now(),
      };
    }),
  cancelMaterialAnalysis: () =>
    set((state) => ({
      materialProcessingMode: null,
      materialAnalysisStatus: "idle",
      materialAnalysisProgress: null,
      materialAnalysisError: null,
      failedChunkIndexes: [],
      materialAnalysisRunId: state.materialAnalysisRunId + 1,
      updatedAt: Date.now(),
    })),
  setUnderstandingDraft: (understandingDraft, understandingRevision) =>
    set({
      understandingDraft,
      understandingRevision,
      understandingDraftDirty: false,
      understandingStatus: "success",
      understandingError: null,
      materialUnderstandingStale: false,
      activeStep: 1,
      updatedAt: Date.now(),
    }),
  updateUnderstandingField: (field, value) =>
    set((state) => ({
      understandingDraft: state.understandingDraft
        ? { ...state.understandingDraft, [field]: value }
        : null,
      understandingDraftDirty: Boolean(state.understandingDraft),
      updatedAt: Date.now(),
    })),
  setUnderstandingStatus: (understandingStatus, understandingError = null) =>
    set({ understandingStatus, understandingError, updatedAt: Date.now() }),
  setGenerationStatus: (generationStatus, generationError = null) =>
    set({
      generationStatus,
      generationError,
      activeStep: generationStatus === "loading" ? 2 : get().activeStep,
      updatedAt: Date.now(),
    }),
  setGenerationResult: (generationResult) =>
    set({
      generationResult,
      generationStatus: generationResult
        ? generationResult.success
          ? "success"
          : "error"
        : "idle",
      generationError: generationResult?.error ?? null,
      activeStep: generationResult ? 2 : get().activeStep,
      updatedAt: Date.now(),
    }),
  setGenerationError: (generationError) =>
    set({
      generationError,
      generationStatus: generationError ? "error" : get().generationStatus,
      updatedAt: Date.now(),
    }),
  resetPptDraft: () =>
    set((state) => ({
      ...state,
      activeStep: 0,
      smartFields: { ...defaultSmartFields },
      materialInputMode: "manual",
      manualRawMaterial: "",
      resolvedMaterialSources: [],
      mergedMaterialText: "",
      mergedMaterialEdited: false,
      materialRevision: 0,
      understandingRevision: null,
      materialUnderstandingStale: false,
      materialProcessingMode: null,
      materialChunkPlan: null,
      chunkUnderstandingDrafts: [],
      failedChunkIndexes: [],
      materialAnalysisStatus: "idle",
      materialAnalysisProgress: null,
      materialAnalysisError: null,
      chunkAnalysisRevision: null,
      materialAnalysisRunId: state.materialAnalysisRunId + 1,
      understandingDraft: null,
      understandingDraftDirty: false,
      understandingStatus: "idle",
      understandingError: null,
      generationStatus: "idle",
      generationError: null,
      generationResult: null,
      updatedAt: Date.now(),
    })),
}));
