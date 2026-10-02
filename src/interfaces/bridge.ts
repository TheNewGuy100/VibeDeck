export interface BridgeErrorDetails {
  [key: string]: unknown;
}

export interface BridgeError {
  code: string;
  operation: string;
  message: string;
  correlationId: string;
  retryable: boolean;
  details?: BridgeErrorDetails;
}

export function normalizeBridgeError(error: unknown, operation: string, correlationId = 'unknown'): BridgeError {
  let value: unknown = error;
  if (typeof value === 'string') {
    try { value = JSON.parse(value); } catch { /* plain native message */ }
  }
  const record = value && typeof value === 'object' ? value as Record<string, unknown> : {};
  const resolvedCorrelationId = typeof record.correlationId === 'string' ? record.correlationId : correlationId;
  const message = error instanceof Error
    ? error.message
    : typeof record.message === 'string' ? record.message
      : typeof record.error === 'string' ? record.error
        : typeof error === 'string' ? error : 'Falha inesperada na comunicação com o bridge nativo.';
  const code = typeof record.code === 'string' ? record.code : 'bridge_error';
  const retryable = typeof record.retryable === 'boolean' ? record.retryable : false;
  const details = record.details && typeof record.details === 'object'
    ? record.details as BridgeErrorDetails
    : typeof record.detail === 'string' ? { detail: record.detail } : undefined;
  return { code, operation: typeof record.operation === 'string' ? record.operation : operation, message, correlationId: resolvedCorrelationId, retryable, ...(details ? { details } : {}) };
}

export function bridgeErrorMessage(error: unknown, operation = 'bridge'): string {
  return normalizeBridgeError(error, operation).message;
}
