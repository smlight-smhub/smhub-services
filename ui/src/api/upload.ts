// Uploads in pieces below the hub's nginx request limit (D9). An interrupted
// piece is sent again from the offset the hub last confirmed.
import { api, ApiError, request, buildUrl } from './client';
import type { Upload } from './types';

/** The contract allows at most 768 KiB per piece. */
export const PIECE = 768 * 1024;
const RETRIES = 4;

export interface UploadProgress {
  sent: number;
  total: number;
}

/** Uploads a file and answers its upload id, for /backups/restore or /files/upload. */
export async function uploadFile(
  file: Blob & { name?: string },
  onProgress: (p: UploadProgress) => void,
  signal?: AbortSignal,
): Promise<string> {
  const started = await api.post('/uploads', { body: { size: file.size, name: file.name } });
  const id = started.id;
  let offset = started.received ?? 0;
  onProgress({ sent: offset, total: file.size });
  while (offset < file.size) {
    const piece = file.slice(offset, offset + PIECE);
    let attempt = 0;
    for (;;) {
      if (signal?.aborted) throw new DOMException('Aborted', 'AbortError');
      try {
        const res = await request('PUT', buildUrl('/uploads/{id}', { id }, { offset }), { body: piece, signal });
        const u = (await res.json()) as Upload;
        offset = u.received > offset ? u.received : offset + piece.size;
        break;
      } catch (e) {
        if ((e as Error).name === 'AbortError') throw e;
        const retryable = e instanceof ApiError && (e.status === 0 || e.status >= 500);
        if (!retryable || ++attempt > RETRIES) throw e;
        await new Promise((r) => setTimeout(r, 500 * 2 ** attempt));
      }
    }
    onProgress({ sent: Math.min(offset, file.size), total: file.size });
  }
  return id;
}
