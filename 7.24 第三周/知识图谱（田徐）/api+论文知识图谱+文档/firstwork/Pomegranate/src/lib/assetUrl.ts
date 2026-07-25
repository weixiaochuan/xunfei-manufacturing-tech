export const KB_ASSET_SCHEME = "kb-asset://";

export function toKbAsset(path: string): string {
  if (!path) return "";
  return path.startsWith(KB_ASSET_SCHEME) ? path : `${KB_ASSET_SCHEME}${path}`;
}

export function parseKbAsset(src: string): string | null {
  return src.startsWith(KB_ASSET_SCHEME) ? src.slice(KB_ASSET_SCHEME.length) : null;
}

export function resolveAssetSrc(src: string, _dataDir?: string): string {
  return parseKbAsset(src) ?? src;
}
