export async function createBlankAndOpen(
  _folderId?: number | null,
  navigate?: (path: string) => void,
  ..._rest: unknown[]
) {
  navigate?.("/notes");
}

export async function importTextFlow(..._args: unknown[]) {
  return null;
}

export async function importPdfsFlow(..._args: unknown[]) {
  return null;
}

export async function importWordFlow(..._args: unknown[]) {
  return null;
}
