import { Agent } from '@earendil-works/pi-agent-core';
import { createModels } from '@earendil-works/pi-ai/models';
import { fauxAssistantMessage, fauxProvider } from '@earendil-works/pi-ai/providers/faux';
import type { ProbeEvent, ProbeRequest } from '../../shared/pi-protocol';

export function createProbe(request: ProbeRequest, emit: (event: ProbeEvent) => void) {
  // This checks the real SDK lifecycle, not the quality or availability of a live model.
  const faux = fauxProvider({ tokensPerSecond: 80, tokenSize: { min: 2, max: 2 } });
  faux.setResponses([fauxAssistantMessage(
    'Pi Core foundation probe: streaming is active. The supplied sample diff is isolated; '
    + 'there are no filesystem tools, shell tools, credentials, or Git write permissions. '
    + 'This is a scripted SDK response, not a real model review.',
  )]);
  const models = createModels();
  models.setProvider(faux.provider);
  const agent = new Agent({
    sessionId: request.context.sessionId,
    identity: { attribution: null },
    initialState: {
      model: faux.getModel(), tools: [], thinkingLevel: 'off',
      systemPrompt: 'Run only the isolated Picket foundation probe. No repository access is granted.',
    },
    streamFn: models.streamSimple.bind(models),
  });
  const envelope = { requestId: request.requestId, context: request.context };
  const unsubscribe = agent.subscribe((event) => {
    if (event.type === 'message_update' && event.assistantMessageEvent.type === 'text_delta') {
      emit({ ...envelope, type: 'textDelta', delta: event.assistantMessageEvent.delta });
    }
  });

  let cancelled = false;
  return {
    cancel: () => { cancelled = true; agent.abort(); },
    async run() {
      emit({ ...envelope, type: 'started' });
      try {
        // Cancellation can arrive in the same stdin chunk before Agent.prompt starts.
        if (cancelled) { emit({ ...envelope, type: 'cancelled' }); return; }
        await agent.prompt('Sample diff only:\n- retryCount = 1\n+ retryCount = 3');
        await agent.waitForIdle();
        const last = agent.state.messages.findLast((message) => message.role === 'assistant');
        if (!last || last.role !== 'assistant') throw new Error('SDK returned no assistant message');
        if (last.stopReason === 'aborted') emit({ ...envelope, type: 'cancelled' });
        else if (last.stopReason === 'error') emit({ ...envelope, type: 'failed', message: last.errorMessage ?? 'SDK error' });
        else emit({ ...envelope, type: 'completed', text: last.content.flatMap((block) =>
          block.type === 'text' ? [block.text] : []).join('') });
      } catch (error) {
        emit({ ...envelope, type: 'failed', message: error instanceof Error ? error.message : 'SDK probe failed' });
      } finally {
        unsubscribe();
      }
    },
  };
}
