import type { AgentTool } from '@earendil-works/pi-agent-core';
import {
  formatMessageUserResponseForModel,
  hasMessageQuestions,
  type MessageUserRequest,
  type MessageUserResponse,
} from '../../../shared/message-user-protocol';
import type { ToolBridge } from './bridge';

type MessageUserToolDetails =
  | MessageUserResponse
  | { displayed: true }
  | { stopAgent: true }
  | { cancelled: true; answers: []; errorCode: string };

export function createMessageUserTool(bridge: ToolBridge): AgentTool<any, MessageUserToolDetails> {
  return {
    name: 'message_user',
    label: 'Message User',
    description: [
      'Communicate with the user in Picket\'s MessageDock using structured rich blocks (`markdown`, `callout`, `facts`, `findings`, `checklist`, `code`, `files`) and optional interactive `questions`.',
      '1) Omit `questions` to publish a live status card or rich final report (non-blocking; Picket promotes the latest published card as the rich outcome card when the action finishes).',
      '2) Include `questions` (1 to 4 items) when user input, confirmation, or authorization is needed (blocking; renders any `blocks` above the choices and waits for the user\'s answer). Do not include an "Other" or "Cancel" option; Picket always provides free-text reply and cancel controls.',
    ].join(' '),
    parameters: {
      type: 'object',
      additionalProperties: false,
      properties: {
        title: { type: 'string', description: 'Concise headline for the MessageDock card (max 120 chars; required when `questions` is omitted).' },
        subtitle: { type: 'string', description: 'Optional short one-line context below the title (max 200 chars).' },
        tone: {
          type: 'string',
          enum: ['info', 'success', 'warning', 'error'],
          description: 'Visual status tone for the card header icon (default: info).',
        },
        blocks: {
          type: 'array',
          maxItems: 12,
          description: '1 to 3 focused structured UI blocks (max 12). Avoid duplicating the same facts across blocks.',
          items: {
            type: 'object',
            required: ['type'],
            properties: {
              type: {
                type: 'string',
                enum: ['markdown', 'callout', 'facts', 'findings', 'checklist', 'code', 'files'],
              },
              content: { type: 'string', description: 'Text content for `markdown`, `callout`, or `code` blocks.' },
              tone: {
                type: 'string',
                enum: ['info', 'success', 'warning', 'danger'],
                description: 'Semantic tone for `callout` blocks.',
              },
              title: { type: 'string', description: 'Optional title for `callout` or `code` blocks.' },
              heading: { type: 'string', description: 'Optional section heading for `facts`, `findings`, `checklist`, or `files`.' },
              language: { type: 'string', description: 'Optional syntax label for `code` blocks.' },
              collapsed: { type: 'boolean', description: 'Collapse `code` block by default (recommended true when >8 lines).' },
              emptyText: { type: 'string', description: 'Shown in `findings` when `items` is empty (e.g. clean review pass).' },
              skipped: {
                type: 'array',
                description: 'Optional skipped files list for `findings` blocks.',
                items: {
                  type: 'object',
                  required: ['path', 'reason'],
                  properties: {
                    path: { type: 'string' },
                    reason: { type: 'string' },
                  },
                },
              },
              items: {
                type: 'array',
                description: 'Structured items for `facts` ({label, value, tone?, mono?}), `findings` ({severity, tag?, title, detail, path?, line?}), `checklist` ({status, label, detail?}), or `files` ({path, line?, badge?}).',
                items: { type: 'object' },
              },
            },
          },
        },
        links: {
          type: 'array',
          maxItems: 6,
          items: {
            type: 'object',
            additionalProperties: false,
            required: ['label', 'url'],
            properties: {
              label: { type: 'string' },
              url: { type: 'string' },
            },
          },
        },
        questions: {
          type: 'array',
          minItems: 1,
          maxItems: 4,
          description: 'Optional 1 to 4 blocking questions. Omit for non-blocking display/report cards.',
          items: {
            type: 'object',
            additionalProperties: false,
            required: ['header', 'question', 'options'],
            properties: {
              id: { type: 'string' },
              header: { type: 'string', description: 'Short tab/chip label (max 24 chars).' },
              question: { type: 'string', description: 'Full question text.' },
              multiSelect: { type: 'boolean', description: 'Allow selecting multiple options (default false).' },
              options: {
                type: 'array',
                minItems: 2,
                maxItems: 4,
                items: {
                  type: 'object',
                  additionalProperties: false,
                  required: ['label', 'description'],
                  properties: {
                    id: { type: 'string' },
                    label: { type: 'string', description: 'Concise choice label (never "Other" or "Cancel").' },
                    description: { type: 'string', description: 'Explanation of the choice or its trade-off.' },
                    recommended: { type: 'boolean' },
                    preview: { type: 'string', description: 'Optional inline preview text (single-select only).' },
                  },
                },
              },
            },
          },
        },
        context: {
          type: 'object',
          additionalProperties: false,
          properties: {
            scope: { type: 'string' },
            affectedFileIds: { type: 'array', items: { type: 'string' } },
            links: {
              type: 'array',
              items: {
                type: 'object',
                additionalProperties: false,
                required: ['label', 'url'],
                properties: {
                  label: { type: 'string' },
                  url: { type: 'string' },
                },
              },
            },
            allowDontAskAgain: { type: 'boolean' },
            dontAskAgainLabel: { type: 'string' },
          },
        },
      },
    },
    executionMode: 'sequential',
    async execute(_toolCallId, params, signal) {
      if (!params || typeof params !== 'object' || Array.isArray(params)) {
        return {
          content: [{ type: 'text', text: 'Invalid message_user arguments: expected an object with `title` and optional `blocks` and/or `questions`.' }],
          details: { cancelled: true, answers: [], errorCode: 'invalid_root' },
          isError: true,
        };
      }
      // Rust (`src-tauri/src/actions/message_user.rs`) is the single authoritative validator for
      // `message_user` blocks, questions, limits, paths, and links.
      const request = params as MessageUserRequest;
      const response = await bridge.invokeHostTool('message_user', params as Record<string, unknown>, signal);
      if (!response.ok || response.isError) {
        return {
          content: [{ type: 'text', text: response.content || 'User message dock was unavailable.' }],
          details: { cancelled: true, answers: [], errorCode: 'host_error' },
          isError: true,
        };
      }
      const detailsRecord = response.details && typeof response.details === 'object'
        ? (response.details as Record<string, unknown>)
        : undefined;
      if (detailsRecord?.stopAgent === true) {
        return {
          content: [{ type: 'text', text: response.content || 'Stopped by host.' }],
          details: { stopAgent: true },
        };
      }
      if (detailsRecord?.displayed === true || !hasMessageQuestions(request)) {
        return {
          content: [{ type: 'text', text: response.content || 'Message displayed to user in MessageDock.' }],
          details: { displayed: true },
        };
      }
      let parsed: MessageUserResponse | undefined;
      if (detailsRecord) {
        parsed = detailsRecord as unknown as MessageUserResponse;
      } else {
        try {
          parsed = JSON.parse(response.content) as MessageUserResponse;
        } catch {
          parsed = undefined;
        }
      }
      if (!parsed || typeof parsed.cancelled !== 'boolean' || !Array.isArray(parsed.answers)) {
        return {
          content: [{ type: 'text', text: response.content }],
          details: { cancelled: true, answers: [], errorCode: 'invalid_host_response' },
          isError: true,
        };
      }
      const modelText = formatMessageUserResponseForModel(request, parsed);
      return {
        content: [{ type: 'text', text: modelText }],
        details: parsed,
      };
    },
  };
}
