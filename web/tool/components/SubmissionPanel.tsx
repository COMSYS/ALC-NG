import { loadSampleFolder } from '../../shared/lib/folder'
import { runAlcNg } from '../../shared/lib/pipeline'
import { SAMPLES } from '../../shared/lib/samples'
import { useStore } from '../../shared/lib/store'
import { IDLE_STATUS, setFlag, setSubmissionOpen, uiStore, type Flags, type Status } from '../../shared/state/ui'
import { ChevronIcon, FolderIcon } from './icons'
import { FolderDrop } from '../../shared/components/FolderDrop'
import { Select } from '../../shared/components/Select'

type BooleanFlag = { [K in keyof Flags]: Flags[K] extends boolean ? K : never }[keyof Flags]

// The switches of the native CLI (see src/cleaner/main.rs and
// src/lib/cleaner/config.rs), with the flag that sets them there.
const SWITCHES: { key: BooleanFlag; label: string; flag: string; help: string }[] = [
  { key: 'tar', label: 'Create archive', flag: '--tar', help: 'Write cleaned.tar.gz for upload. Spaces in file names become underscores and the references are updated.' },
  { key: 'compare', label: 'Compare PDFs', flag: '--compare', help: 'Compile the cleaned sources as well and compare both PDFs page by page.' },
  { key: 'keepBib', label: 'Keep .bib files', flag: '--keep-bib', help: 'Keep the bibliography sources next to the generated .bbl.' },
  { key: 'cleanClasses', label: 'Clean .sty and .cls files', flag: '--clean-classes', help: 'Also remove comments from style and class files.' },
  { key: 'resizeImages', label: 'Resize images', flag: '--resize-images', help: 'Scale raster images down to the image size below.' },
  { key: 'noZzrm', label: 'Ignore 00README', flag: '--no-zzrm', help: 'Do not read processing instructions from a 00README file.' },
  { key: 'skipWatermark', label: 'No watermark', flag: '--skip-watermark', help: 'Do not append the “Processed with alc-ng” comment to the main files.' },
  { key: 'force', label: 'Continue on errors', flag: '--force', help: 'Keep cleaning after an error instead of stopping.' },
]

function StatusLine({ status }: { status: Status }) {
  return (
    <p className={`status ${status.tone}`} role="status">
      {status.text}
    </p>
  )
}

function Options({ flags }: { flags: Flags }) {
  return (
    <details className="options">
      <summary>
        <ChevronIcon />
        Options
      </summary>
      <div className="optionList">
        {SWITCHES.map(({ key, label, flag, help }) => (
          <label className="check" key={key}>
            <input type="checkbox" checked={flags[key]} onChange={(ev) => setFlag(key, ev.target.checked)} />
            <span className="checkText">
              {label}
              <span className="flag">{flag}</span>
              <span className="help">{help}</span>
            </span>
          </label>
        ))}
      </div>
      <div className="valueGrid">
        <div className="field">
          <label htmlFor="opt-im-size">Image size</label>
          <input
            id="opt-im-size"
            className="input"
            type="number"
            min={16}
            max={8192}
            step={1}
            value={flags.imSize}
            disabled={!flags.resizeImages}
            onChange={(ev) => setFlag('imSize', ev.target.value === '' ? 512 : Math.min(8192, Math.max(16, Number(ev.target.value))))}
          />
          <p className="help">
            Longest side in pixels, <code>--im-size</code>
          </p>
        </div>
        <div className="field">
          <label htmlFor="opt-diff-color">Diff colour</label>
          <span className="colorField">
            <input
              id="opt-diff-color"
              type="color"
              value={flags.diffColor}
              disabled={!flags.compare}
              onChange={(ev) => setFlag('diffColor', ev.target.value)}
            />
            <code>{flags.diffColor}</code>
          </span>
          <p className="help">
            Marks changed pixels, <code>--diff-color</code>
          </p>
        </div>
        <div className="field">
          <label htmlFor="opt-engine">TeX engine</label>
          <select id="opt-engine" className="input" value={flags.engine} onChange={(ev) => setFlag('engine', ev.target.value as Flags['engine'])}>
            <option value="auto">Automatic</option>
            <option value="pdftex">pdfLaTeX</option>
            <option value="xetex">XeLaTeX</option>
          </select>
        </div>
        <div className="field">
          <label htmlFor="opt-log">Log level</label>
          <select id="opt-log" className="input" value={flags.logLevel} onChange={(ev) => setFlag('logLevel', ev.target.value as Flags['logLevel'])}>
            <option value="off">Off</option>
            <option value="info">Info</option>
            <option value="trace">Trace</option>
          </select>
        </div>
        <p className="help wide">
          Automatic uses XeLaTeX when a 00README asks for it or the sources load fontspec, unicode-math, polyglossia or xeCJK.
        </p>
      </div>
    </details>
  )
}

export function SubmissionPanel() {
  const ui = useStore(uiStore, (s) => s)
  const canRun = !!ui.folder && ui.mainSel.length > 0 && !ui.running && !ui.sampleLoading
  const open = ui.submissionOpen

  return (
    <section className="sideSection">
      <div className="sectionHead">
        <button type="button" className="sectionToggle" aria-expanded={open} onClick={() => setSubmissionOpen(!open)}>
          <ChevronIcon />
          Submission
        </button>
        {!open && ui.folder && <span className="count">{ui.folder.label}</span>}
      </div>
      {/* hidden rather than unmounted, so the options keep their open state */}
      <div hidden={!open} className="sideSectionBody">
        <FolderDrop icon={<FolderIcon />} folder={ui.folder} disabled={ui.running || ui.sampleLoading} running={ui.running} />

        <div className="field">
          <span className="fieldLabel">Or load a sample</span>
          <Select
            icon={<ChevronIcon />}
            ariaLabel="Sample submission"
            placeholder="Choose a sample"
            value={ui.sample}
            loading={ui.sampleLoading}
            disabled={ui.running || ui.sampleLoading}
            options={SAMPLES.map((s, i) => ({ value: i, label: s.pick, hint: s.blurb }))}
            onChange={(i) => void loadSampleFolder(i)}
          />
        </div>

        {ui.folder && ui.mainFiles.length > 1 && (
          // Several candidates map native alc-ng's --main-files list: the run
          // cleans around every checked main.
          <fieldset>
            <legend>Main files</legend>
            <div className="checkList">
              {ui.mainFiles.map((o) => (
                <label className="check fileCheck" key={o.value} title={o.label}>
                  <input
                    type="checkbox"
                    checked={ui.mainSel.includes(o.value)}
                    onChange={(ev) =>
                      uiStore.set((s) => ({
                        // keep the candidates' ranked order
                        mainSel: s.mainFiles.map((m) => m.value).filter((v) => (v === o.value ? ev.target.checked : s.mainSel.includes(v))),
                      }))
                    }
                  />
                  <span>{o.label}</span>
                </label>
              ))}
            </div>
            <p className="help">Every checked file is compiled, and the cleaner keeps what any of them uses. The first one is shown and compared.</p>
          </fieldset>
        )}

        <Options flags={ui.flags} />

        {/* the idle prompt is the workspace's empty state already */}
        {/* while running, the step list in the workspace reports progress */}
        {ui.status && ui.status !== IDLE_STATUS && !ui.running && <StatusLine status={ui.status} />}

        <button type="button" className={`button primary block${ui.running ? ' busy' : ''}`} disabled={!canRun} onClick={() => void runAlcNg()}>
          {ui.running && <span className="spinner" aria-hidden="true" />}
          {ui.running ? 'Running…' : 'Run alc-ng'}
        </button>
      </div>
    </section>
  )
}
