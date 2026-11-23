import { createRoot } from 'react-dom/client';
import { Workbench } from '../Workbench';
import { fixture, scenarios } from './fixtures';
import type { Scenario } from './fixtures';

// This is a separate Vite development entry, never imported by src/main.tsx.
const parameters = new URLSearchParams(window.location.search);
const scenario = parameters.get('scenario');
const rootElement = document.getElementById('root')!;
const root = createRoot(rootElement);
if (parameters.has('verify')) {
  void import('./browser-checks').then(({ verifyWorkbench }) => verifyWorkbench(root)).then((checks) => {
    const output = document.createElement('pre'); output.dataset.workbenchResult = 'passed'; output.textContent = JSON.stringify(checks); document.body.append(output);
  }).catch((error: unknown) => {
    const output = document.createElement('pre'); output.dataset.workbenchResult = 'failed'; output.textContent = String(error); document.body.append(output);
  });
} else if (scenario === 'foundation') {
  void import('../../foundation/Foundation').then(({ Foundation }) => root.render(<Foundation />));
} else {
  const selected: Scenario = scenarios.includes(scenario as Scenario) ? scenario as Scenario : 'question';
  root.render(<Workbench initialState={fixture(selected)} />);
}
