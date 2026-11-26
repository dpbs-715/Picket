import type { AgentTool } from '@earendil-works/pi-agent-core';
import {
  formatAskUserQuestionResponseForModel,
  validateAskUserQuestionRequest,
  type AskUserQuestionResponse,
} from '../../../shared/ask-user-protocol';
import type { ToolBridge } from './bridge';

export function createAskUserQuestionTool(bridge: ToolBridge): AgentTool<any, AskUserQuestionResponse | { cancelled: true; answers: []; errorCode: string }> {
  return {
    name: 'ask_user_question',
    label: 'Ask User Question',
    description: 'Present 1 to 4 structured questions in Picket\'s unified dialog when user input, scope grouping, or explicit authorization is required. Do not include an "Other" option; the dialog always provides free-text input.',
    parameters: {
      type: 'object',
      additionalProperties: false,
      required: ['questions'],
      properties: {
        questions: {
          type: 'array',
          minItems: 1,
          maxItems: 4,
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
      const validation = validateAskUserQuestionRequest(params);
      if (!validation.ok) {
        return {
          content: [{ type: 'text', text: `Invalid ask_user_question arguments (${validation.code}): ${validation.message}` }],
          details: { cancelled: true, answers: [], errorCode: validation.code },
          isError: true,
        };
      }
      const response = await bridge.invokeHostTool('ask_user_question', validation.request as unknown as Record<string, unknown>, signal);
      if (!response.ok || response.isError) {
        return {
          content: [{ type: 'text', text: response.content || 'User dialog was unavailable.' }],
          details: { cancelled: true, answers: [], errorCode: 'host_error' },
          isError: true,
        };
      }
      let parsed: AskUserQuestionResponse | undefined;
      if (response.details && typeof response.details === 'object') {
        parsed = response.details as AskUserQuestionResponse;
      } else {
        try {
          parsed = JSON.parse(response.content) as AskUserQuestionResponse;
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
      const modelText = formatAskUserQuestionResponseForModel(validation.request, parsed);
      return {
        content: [{ type: 'text', text: modelText }],
        details: parsed,
      };
    },
  };
}
