import { createContext, useContext } from "react";
import type {
  EditorConfig,
  EditorCore,
  EditorRegistry,
  EditorRegistryEntry,
} from "@/types/editor";

type EditorFactory = (config: EditorConfig) => EditorCore;
type RegistryInput = EditorRegistryEntry | EditorFactory;

class MinimalEditorRegistry implements EditorRegistry {
  private entries = new Map<string, EditorRegistryEntry>();

  register(editor: EditorRegistryEntry): void;
  register(type: string, adapter: RegistryInput): void;
  register(
    editorOrType: EditorRegistryEntry | string,
    adapter?: RegistryInput,
  ): void {
    if (typeof editorOrType === "string") {
      if (!adapter) return;
      const entry =
        typeof adapter === "function"
          ? this.createEntry(editorOrType, adapter)
          : { ...adapter, id: editorOrType };
      this.entries.set(editorOrType, entry);
      return;
    }

    this.entries.set(editorOrType.id, editorOrType);
  }

  unregister(id: string): void {
    this.entries.delete(id);
  }

  get(type: string): EditorRegistryEntry | undefined {
    return this.entries.get(type);
  }

  has(type: string): boolean {
    return this.entries.has(type);
  }

  list(): EditorRegistryEntry[] {
    return this.getEditors();
  }

  getEditors(): EditorRegistryEntry[] {
    return Array.from(this.entries.values());
  }

  getEditor(id: string): EditorRegistryEntry | undefined {
    return this.entries.get(id);
  }

  getEditorForFileType(fileType: string): EditorRegistryEntry | undefined {
    const normalized = fileType.replace(/^\./, "").toLowerCase();
    return this.getEditors().find((entry) =>
      entry.supportedFileTypes.some(
        (supported) => supported.toLowerCase() === normalized,
      ),
    );
  }

  getEditorsByFeature(feature: string): EditorRegistryEntry[] {
    return this.getEditors().filter((entry) =>
      entry.features.includes(feature),
    );
  }

  private createEntry(
    id: string,
    factory: EditorFactory,
  ): EditorRegistryEntry {
    return {
      id,
      name: id,
      supportedFileTypes: [],
      features: [],
      factory,
    };
  }
}

export type EditorRegistryInstance = MinimalEditorRegistry;

export const editorRegistry = new MinimalEditorRegistry();

const EditorRegistryContext =
  createContext<EditorRegistryInstance>(editorRegistry);

export function EditorRegistryProvider({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <EditorRegistryContext.Provider value={editorRegistry}>
      {children}
    </EditorRegistryContext.Provider>
  );
}

export function useEditorRegistry(): EditorRegistryInstance {
  return useContext(EditorRegistryContext);
}

