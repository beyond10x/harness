import {useState, type ReactNode} from 'react';
import Link from '@docusaurus/Link';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';

import styles from './index.module.css';

const loopSteps = [
  ['01', 'Assemble', 'instruction + conversation + live catalogue'],
  ['02', 'Stream', 'one stateless provider turn'],
  ['03', 'Resolve', 'tool name → neutral operation → risk'],
  ['04', 'Gate', 'approve, refuse, or run inside the boundary'],
  ['05', 'Record', 'outcome + usage + cost + next turn'],
];

const truths = [
  {
    title: 'The toolset follows the machine',
    text: 'Four read tools by default. Writes appear behind a guarded workspace. Execution appears only when the host can confine an explicitly allowed argv.',
    signal: 'capability, not prompt',
    to: '/docs/concepts/tools-and-approvals',
  },
  {
    title: 'A refusal reaches the model',
    text: 'Denied approval, a blocked path, an oversized result, or an unpublished tool becomes a failed outcome the next turn can reason about.',
    signal: 'no silent success',
    to: '/docs/concepts/agent-loop',
  },
  {
    title: 'Unknown never becomes zero',
    text: 'Missing usage stays absent. Cost exists only under a dated rate card. A bound the loop cannot measure is refused before a paid turn.',
    signal: 'accounting with provenance',
    to: '/docs/reference/cli#budgets-and-accounting',
  },
  {
    title: 'A run leaves a useful record',
    text: 'Local sessions survive failure and resume opaque provider items verbatim. JSONL exposes the same event stream the terminal reads.',
    signal: 'conversation + events',
    to: '/docs/guides/sessions-and-events',
  },
];

const quickCommand = 'cargo run --locked -p b10x-harness-cli -- tools --workspace .';

const pathways = [
  {
    number: '01',
    label: 'Make a first run',
    detail: 'read-only by default',
    to: '/docs/getting-started',
  },
  {
    number: '02',
    label: 'Trace the loop',
    detail: 'turns, tools, outcomes',
    to: '/docs/concepts/agent-loop',
  },
  {
    number: '03',
    label: 'Bound effects',
    detail: 'risk and confinement',
    to: '/docs/concepts/security-boundary',
  },
  {
    number: '04',
    label: 'Embed the contract',
    detail: 'CLI and provider wires',
    to: '/docs/reference/cli',
  },
];

function QuickCommand(): ReactNode {
  const [copyState, setCopyState] = useState<'idle' | 'copied' | 'failed'>('idle');

  async function copyCommand() {
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(quickCommand);
      } else {
        const copyTarget = document.createElement('textarea');
        copyTarget.value = quickCommand;
        copyTarget.style.position = 'fixed';
        copyTarget.style.opacity = '0';
        document.body.append(copyTarget);
        copyTarget.select();
        const copied = document.execCommand('copy');
        copyTarget.remove();
        if (!copied) throw new Error('Copy command was unavailable');
      }
      setCopyState('copied');
      window.setTimeout(() => setCopyState('idle'), 1800);
    } catch {
      setCopyState('failed');
    }
  }

  const copyLabel =
    copyState === 'copied' ? 'Copied' : copyState === 'failed' ? 'Copy failed' : 'Copy';

  return (
    <div className={styles.quickCommand}>
      <span className={styles.commandPrompt} aria-hidden="true">$</span>
      <code>{quickCommand}</code>
      <button type="button" onClick={copyCommand} aria-label="Copy the tool inspection command">
        <span aria-live="polite">{copyLabel}</span>
        <i aria-hidden="true" />
      </button>
    </div>
  );
}

function RunPanel(): ReactNode {
  const [wire, setWire] = useState<'responses' | 'messages'>('responses');
  const wireLabel = wire === 'responses' ? 'openai-responses' : 'anthropic-messages';

  return (
    <aside className={styles.runPanel} aria-label="A Harness run moving through the approval gate">
      <div className={styles.panelBar}>
        <span>run / 018c…9f</span>
        <span className={styles.live}><i /> streaming</span>
      </div>
      <div className={styles.wirePicker} role="tablist" aria-label="Preview a provider wire">
        <span>PROVIDER WIRE</span>
        <div>
          <button
            type="button"
            role="tab"
            aria-selected={wire === 'responses'}
            onClick={() => setWire('responses')}>
            Responses
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={wire === 'messages'}
            onClick={() => setWire('messages')}>
            Messages
          </button>
        </div>
      </div>
      <div className={styles.panelModel}>
        <span>MODEL</span>
        <strong>model-alias</strong>
        <small aria-live="polite">{wireLabel} · turn 02</small>
      </div>
      <ol className={styles.trace}>
        <li className={styles.traceDone}>
          <span>01</span>
          <div><strong>file_read</strong><small>low · admitted</small></div>
          <b>ok</b>
        </li>
        <li className={styles.traceActive}>
          <span>02</span>
          <div><strong>file_edit</strong><small>medium · needs a decision</small></div>
          <b>?</b>
        </li>
        <li>
          <span>03</span>
          <div><strong>tool outcome</strong><small>the model learns what happened</small></div>
          <b>·</b>
        </li>
      </ol>
      <div className={styles.gate}>
        <span>APPROVAL GATE</span>
        <p>Risk is above the unattended ceiling. No effect has happened.</p>
      </div>
      <div className={styles.panelFoot}>
        <span>12 turns max</span>
        <span>180 s</span>
        <span>session on</span>
      </div>
    </aside>
  );
}

export default function Home(): ReactNode {
  return (
    <Layout
      title="A provider-neutral agent loop with explicit effects"
      description="Harness owns model turns, tool round trips, approvals, budgets and local sessions across OpenAI Responses and Anthropic Messages endpoints.">
      <main>
        <header className={styles.hero}>
          <div className={styles.heroGlow} />
          <div className={`container ${styles.heroGrid}`}>
            <div className={styles.heroCopy}>
              <p className={styles.eyebrow}><span /> ONE LOOP / TWO WIRES / EXPLICIT EFFECTS</p>
              <Heading as="h1">The agent loop <em>you can account for.</em></Heading>
              <p className={styles.lede}>
                Talk to model APIs directly. Publish only the tools this machine can perform.
                Decide consequential calls before they happen—and keep the conversation, usage,
                cost and stop reason when the run is over.
              </p>
              <div className={styles.actions}>
                <Link className={styles.primaryAction} to="/docs/getting-started">
                  Run it read-only <span aria-hidden="true">↗</span>
                </Link>
                <Link className={styles.secondaryAction} to="/docs/concepts/agent-loop">
                  Understand the loop
                </Link>
              </div>
              <div className={styles.commandWrap}>
                <span>Inspect the safe default · no model call</span>
                <QuickCommand />
              </div>
              <div className={styles.metrics} aria-label="Harness at a glance">
                <div><strong>2</strong><span>provider wires</span></div>
                <div><strong>3</strong><span>shells, one loop</span></div>
                <div><strong>0</strong><span>ambient credentials</span></div>
              </div>
            </div>
            <RunPanel />
          </div>
        </header>

        <nav className={styles.pathways} aria-label="Explore Harness documentation">
          <div className="container">
            <span className={styles.pathwaysLabel}>CHOOSE A PATH</span>
            <div className={styles.pathwaysGrid}>
              {pathways.map((path) => (
                <Link to={path.to} key={path.number}>
                  <small>{path.number}</small>
                  <span><strong>{path.label}</strong><em>{path.detail}</em></span>
                  <b aria-hidden="true">↗</b>
                </Link>
              ))}
            </div>
          </div>
        </nav>

        <section className={styles.thesis}>
          <div className="container">
            <p className={styles.sectionLabel}>THE BOUNDARY</p>
            <Heading as="h2">model turn → proposed calls → gate → observed outcomes</Heading>
            <div className={styles.thesisGrid}>
              <p>
                Harness is not a wrapper around somebody else&apos;s coding-agent binary. It owns
                turn assembly, streaming, tool round trips, approval, compaction and budgets. The
                model provider owns one turn; the tool port owns one admitted operation.
              </p>
              <p>
                That makes the effect boundary inspectable. A tool absent from the machine is
                absent from the model. A call above the risk ceiling waits. A refusal enters the
                conversation as a refusal—not as an error that leaves the model believing it ran.
              </p>
            </div>
          </div>
        </section>

        <section className={styles.loop}>
          <div className="container">
            <div className={styles.sectionHead}>
              <div>
                <p className={styles.sectionLabel}>ONE TURN AT A TIME</p>
                <Heading as="h2">A small loop with hard edges.</Heading>
              </div>
              <Link to="/docs/concepts/agent-loop">Read the execution model →</Link>
            </div>
            <ol className={styles.loopRail}>
              {loopSteps.map(([number, title, text]) => (
                <li key={number}>
                  <span>{number}</span>
                  <Heading as="h3">{title}</Heading>
                  <p>{text}</p>
                </li>
              ))}
            </ol>
          </div>
        </section>

        <section className={styles.capabilities}>
          <div className={`container ${styles.capGrid}`}>
            <div className={styles.capCopy}>
              <p className={styles.sectionLabel}>CAPABILITY LADDER</p>
              <Heading as="h2">Start with sight. Add effects only where they can land.</Heading>
              <p>
                The same command grows from four read tools to a writing and executing agent only
                through a named substrate boundary. Inspect the exact catalogue without contacting
                a model.
              </p>
              <div className={styles.inlineActions}>
                <Link to="/docs/guides/confinement">Configure confinement ↗</Link>
                <Link to="/docs/concepts/security-boundary">Read the security boundary</Link>
              </div>
            </div>
            <div className={styles.ladder} aria-label="The three Harness capability tiers">
              <article>
                <span>01 / DEFAULT</span>
                <Heading as="h3">Read</Heading>
                <p>file_read · dir_list · search · find</p>
                <b>workspace-contained</b>
              </article>
              <article>
                <span>02 / GUARDED IO</span>
                <Heading as="h3">Write</Heading>
                <p>file_write · file_edit</p>
                <b>medium risk · approval</b>
              </article>
              <article>
                <span>03 / CONFINED EXEC</span>
                <Heading as="h3">Run</Heading>
                <p>explicit program · argv only</p>
                <b>high risk · approval</b>
              </article>
            </div>
          </div>
        </section>

        <section className={styles.truths}>
          <div className="container">
            <div className={styles.sectionHead}>
              <div>
                <p className={styles.sectionLabel}>WHAT A RUN CAN TELL YOU</p>
                <Heading as="h2">The record explains the outcome.</Heading>
              </div>
              <Link to="/docs/guides/sessions-and-events">Sessions and events →</Link>
            </div>
            <div className={styles.truthGrid}>
              {truths.map((truth, index) => (
                <Link className={styles.truthCard} to={truth.to} key={truth.title}>
                  <span>0{index + 1}</span>
                  <Heading as="h3">{truth.title}</Heading>
                  <p>{truth.text}</p>
                  <b>{truth.signal} →</b>
                </Link>
              ))}
            </div>
          </div>
        </section>

        <section className={styles.closing}>
          <div className={`container ${styles.closingGrid}`}>
            <div>
              <p className={styles.sectionLabel}>PRE-V1 / BUILD FROM SOURCE</p>
              <Heading as="h2">Begin with a run that can only read.</Heading>
            </div>
            <div className={styles.closingAction}>
              <p>
                Inspect the catalogue, name the endpoint and credential source, set a budget, and
                make the smallest useful claim about one workspace.
              </p>
              <Link className={styles.primaryAction} to="/docs/getting-started">
                Open the quickstart <span aria-hidden="true">↗</span>
              </Link>
            </div>
          </div>
        </section>
      </main>
    </Layout>
  );
}
