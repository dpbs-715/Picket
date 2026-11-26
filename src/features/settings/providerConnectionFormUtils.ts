import type {
  CreateProviderConnectionInput,
  ListModelsInput,
  ProviderRevealKeyReply,
  ProviderSaveReply,
  ProviderUpdateReply,
  RemoteModelOption,
  TestConnectionInput,
  UpdateProviderConnectionInput,
} from '../../../shared/model-protocol';
import type { OverlayVariant } from '../../ui/controls';
import type { ModelConfiguration, ProviderConnection } from './modelProviders';

export const REVEAL_AUTO_HIDE_MS = 30_000;

export function sanitizeModelError(error: unknown, secrets: readonly string[], fallback: string): string {
  let text = error instanceof Error ? error.message : typeof error === 'string' ? error : fallback;
  const candidates = secrets.flatMap((item) => [item, item.trim()]).filter((item) => item.length > 0);
  for (const secret of candidates) {
    text = text.split(secret).join('[REDACTED]');
    if (secret.length >= 4) {
      const encoded = encodeURIComponent(secret);
      if (encoded !== secret) text = text.split(encoded).join('[REDACTED]');
    }
  }
  return text.trim() || fallback;
}

export function parseOptionalPositiveInt(raw: string): number | 'invalid' | undefined {
  const trimmed = raw.trim();
  if (!trimmed) return undefined;
  if (!/^\d+$/.test(trimmed)) return 'invalid';
  const parsed = Number(trimmed);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) return 'invalid';
  return parsed;
}

export type CreateProviderConnectionFormProps = {
  mode: 'create';
  keepActiveModel?: boolean;
  overlayVariant?: OverlayVariant;
  covered?: boolean;
  depth?: number;
  onBack?(): void;
  backLabel?: string;
  closeDisabled?: boolean;
  onClose(): void;
  onCreated(reply: ProviderSaveReply): void;
  test(input: TestConnectionInput): Promise<string>;
  listModels?(input: ListModelsInput): Promise<RemoteModelOption[]>;
  create(input: CreateProviderConnectionInput): Promise<ProviderSaveReply>;
};

export type EditProviderConnectionFormProps = {
  mode: 'edit';
  overlayVariant?: OverlayVariant;
  covered?: boolean;
  depth?: number;
  onBack?(): void;
  backLabel?: string;
  closeDisabled?: boolean;
  connection: ProviderConnection;
  childModels: readonly ModelConfiguration[];
  onClose(): void;
  onUpdated(reply: ProviderUpdateReply): void;
  onKeyHintResolved?: (connectionId: string, keyHint: string) => void;
  test(input: TestConnectionInput): Promise<string>;
  update(input: UpdateProviderConnectionInput): Promise<ProviderUpdateReply>;
  revealApiKey?: (connectionId: string) => Promise<ProviderRevealKeyReply>;
};
