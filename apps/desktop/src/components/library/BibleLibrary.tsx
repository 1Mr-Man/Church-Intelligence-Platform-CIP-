/**
 * Bible Library (Phase 3.6): search, browse (Testament -> Book -> Chapter
 * -> Verses), save, and reuse against the real, complete BSB dataset - not
 * a duplicate of the existing Manual Bible Search in Diagnostics, which
 * stays exactly as it was (a quick reference/text box for mid-service
 * use). This is the deeper "go find/explore Scripture" surface the
 * operator reaches from top-level navigation, not from inside a live
 * service.
 *
 * Every action here calls an existing command (`searchBible`,
 * `previewScripture`, `createManualPresentation`) or one of the three
 * narrowly-scoped Phase 3.6 additions (`listBibleBooks`, `saveScripture`,
 * `listSavedScriptures`/`deleteSavedScripture`) - see
 * docs/phase-3-6-church-libraries.md for why each one was judged
 * necessary rather than reusable. Preparing a scripture here uses the
 * exact same `createManualPresentation` command the old Manual Bible
 * Search already used - it is still an explicit, operator-initiated
 * action; nothing here ever displays anything automatically (spec's hard
 * safety rule).
 */
import { useEffect, useState } from "react";
import type { BibleBook, BibleSearchResult, BibleTranslation, PresentationPreview, SavedScripture } from "../../domain";
import * as commands from "../../lib/commands";
import { parseVerseRange, referenceFor } from "../../lib/libraryHelpers";

type Tab = "browse" | "search" | "saved";

export function BibleLibrary() {
  const [tab, setTab] = useState<Tab>("browse");
  const [translations, setTranslations] = useState<BibleTranslation[]>([]);
  const [translationId, setTranslationId] = useState<string>("");
  const [books, setBooks] = useState<BibleBook[]>([]);
  const [selectedBook, setSelectedBook] = useState<BibleBook | null>(null);
  const [selectedChapter, setSelectedChapter] = useState<number | null>(null);
  const [chapterVerses, setChapterVerses] = useState<BibleSearchResult[]>([]);
  const [rangeFrom, setRangeFrom] = useState("");
  const [rangeTo, setRangeTo] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<BibleSearchResult[]>([]);
  const [saved, setSaved] = useState<SavedScripture[]>([]);
  const [previews, setPreviews] = useState<Record<string, PresentationPreview>>({});
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  useEffect(() => {
    commands.listBibleTranslations().then(setTranslations).catch(() => {});
    commands.listSavedScriptures().then(setSaved).catch(() => {});
  }, []);

  useEffect(() => {
    commands
      .listBibleBooks(translationId || undefined)
      .then(setBooks)
      .catch((e) => setError(String(e)));
  }, [translationId]);

  const withBusy = async (key: string, action: () => Promise<void>) => {
    setBusy(key);
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const openChapter = (book: BibleBook, chapter: number) => {
    setSelectedBook(book);
    setSelectedChapter(chapter);
    setRangeFrom("");
    setRangeTo("");
    void withBusy("open-chapter", async () => {
      const results = await commands.searchBible(`${book.code} ${chapter}`, translationId || undefined);
      setChapterVerses(results);
    });
  };

  const runSearch = () => {
    if (!searchQuery.trim()) return;
    void withBusy("search", async () => {
      const results = await commands.searchBible(searchQuery.trim(), translationId || undefined);
      setSearchResults(results);
    });
  };

  const saveVerse = (result: BibleSearchResult, verseEnd?: number) => {
    const key = `save-${result.book}-${result.chapter}-${result.verse}`;
    void withBusy(key, async () => {
      const reference = referenceFor(result.book, result.chapter, result.verse, verseEnd);
      const record = await commands.saveScripture({
        translationId: result.translationId,
        book: result.book,
        chapter: result.chapter,
        verseStart: result.verse,
        verseEnd: verseEnd ?? null,
        referenceDisplay: reference,
      });
      setSaved((prev) => [record, ...prev]);
      setStatus(`Saved ${reference}`);
    });
  };

  const preview = (reference: string) => {
    void withBusy(`preview-${reference}`, async () => {
      const p = await commands.previewScripture(reference, translationId || undefined);
      setPreviews((prev) => ({ ...prev, [reference]: p }));
    });
  };

  const prepare = (reference: string) => {
    void withBusy(`prepare-${reference}`, async () => {
      await commands.createManualPresentation(reference, translationId || undefined);
      setStatus(`Prepared ${reference} - open Live Service to display it.`);
    });
  };

  const removeSaved = (id: string) => {
    void withBusy(`delete-${id}`, async () => {
      await commands.deleteSavedScripture(id);
      setSaved((prev) => prev.filter((s) => s.id !== id));
    });
  };

  const saveRange = () => {
    if (!selectedBook || !selectedChapter) return;
    const range = parseVerseRange(rangeFrom, rangeTo);
    if (!range) {
      setError("Enter a valid verse range (from <= to).");
      return;
    }
    void withBusy("save-range", async () => {
      const reference = referenceFor(selectedBook.code, selectedChapter, range.from, range.to);
      const record = await commands.saveScripture({
        translationId: translationId || "BSB",
        book: selectedBook.code,
        chapter: selectedChapter,
        verseStart: range.from,
        verseEnd: range.to,
        referenceDisplay: reference,
      });
      setSaved((prev) => [record, ...prev]);
      setStatus(`Saved ${reference}`);
    });
  };

  const prepareRange = () => {
    if (!selectedBook || !selectedChapter) return;
    const range = parseVerseRange(rangeFrom, rangeTo);
    if (!range) {
      setError("Enter a valid verse range (from <= to).");
      return;
    }
    prepare(referenceFor(selectedBook.code, selectedChapter, range.from, range.to));
  };

  const isBusy = (key: string) => busy === key;
  const oldTestament = books.filter((b) => b.testament === "old");
  const newTestament = books.filter((b) => b.testament === "new");

  function renderVerseCard(result: BibleSearchResult) {
    const key = `${result.translationId}-${result.book}-${result.chapter}-${result.verse}`;
    return (
      <li key={key} className="library-card library-card--bible">
        <div className="library-card__header">
          <strong>{result.reference}</strong>
          <span className="library-card__meta">{result.translationId}</span>
        </div>
        <p className="library-card__text">{result.text}</p>
        <div className="library-card__actions">
          <button type="button" disabled={isBusy(`preview-${result.reference}`)} onClick={() => preview(result.reference)}>
            Preview
          </button>
          <button
            type="button"
            className="op-button--primary"
            disabled={isBusy(`prepare-${result.reference}`)}
            onClick={() => prepare(result.reference)}
          >
            Prepare
          </button>
          <button
            type="button"
            disabled={isBusy(`save-${result.book}-${result.chapter}-${result.verse}`)}
            onClick={() => saveVerse(result)}
          >
            Save
          </button>
        </div>
        {previews[result.reference] && (
          <div className="live-brain__preview-pane">
            <p className="live-brain__label">Preview &mdash; {previews[result.reference].slide.template}</p>
            <p>
              <strong>{previews[result.reference].slide.heading}</strong>
            </p>
            {previews[result.reference].slide.bodyLines.map((line, i) => (
              <p key={i}>{line}</p>
            ))}
          </div>
        )}
      </li>
    );
  }

  return (
    <div className="library-page library-page--bible">
      <header className="library-page__header">
        <div>
          <p className="library-page__eyebrow">Bible Library</p>
          <h1>Scripture</h1>
        </div>
        <select value={translationId} onChange={(e) => setTranslationId(e.target.value)} aria-label="Translation">
          <option value="">Default translation</option>
          {translations.map((t) => (
            <option key={t.id} value={t.id}>
              {t.name}
            </option>
          ))}
        </select>
      </header>

      {books.length > 0 ? (
        <p className="library-page__status-line">
          {books.length} book{books.length === 1 ? "" : "s"} available for search and browsing.
        </p>
      ) : (
        <p className="library-page__status-line">No Bible content installed for this translation yet.</p>
      )}

      {error && (
        <p className="live-brain__error" role="alert">
          {error}
        </p>
      )}
      {status && <p className="library-page__notice">{status}</p>}

      <nav className="library-tabs" role="tablist" aria-label="Bible Library sections">
        <button type="button" aria-pressed={tab === "browse"} onClick={() => setTab("browse")}>
          Browse
        </button>
        <button type="button" aria-pressed={tab === "search"} onClick={() => setTab("search")}>
          Search
        </button>
        <button type="button" aria-pressed={tab === "saved"} onClick={() => setTab("saved")}>
          Saved ({saved.length})
        </button>
      </nav>

      {tab === "browse" && (
        <section className="library-panel">
          {!selectedBook ? (
            <>
              <h2>Old Testament</h2>
              <div className="library-book-grid">
                {oldTestament.map((b) => (
                  <button key={b.code} type="button" className="library-book-tile" onClick={() => setSelectedBook(b)}>
                    {b.name}
                  </button>
                ))}
              </div>
              <h2>New Testament</h2>
              <div className="library-book-grid">
                {newTestament.map((b) => (
                  <button key={b.code} type="button" className="library-book-tile" onClick={() => setSelectedBook(b)}>
                    {b.name}
                  </button>
                ))}
              </div>
            </>
          ) : !selectedChapter ? (
            <>
              <button type="button" onClick={() => setSelectedBook(null)}>
                &larr; All books
              </button>
              <h2>{selectedBook.name}</h2>
              <div className="library-chapter-grid">
                {Array.from({ length: selectedBook.chapterCount }, (_, i) => i + 1).map((c) => (
                  <button key={c} type="button" className="library-chapter-tile" onClick={() => openChapter(selectedBook, c)}>
                    {c}
                  </button>
                ))}
              </div>
            </>
          ) : (
            <>
              <button
                type="button"
                onClick={() => {
                  setSelectedChapter(null);
                  setChapterVerses([]);
                }}
              >
                &larr; {selectedBook.name}
              </button>
              <h2>
                {selectedBook.name} {selectedChapter}
              </h2>
              <div className="library-range-tool">
                <span className="live-brain__label">Save or prepare a range within this chapter</span>
                <div className="live-brain__row">
                  <input
                    value={rangeFrom}
                    onChange={(e) => setRangeFrom(e.target.value)}
                    placeholder="From verse"
                    inputMode="numeric"
                    aria-label="Range start verse"
                  />
                  <input
                    value={rangeTo}
                    onChange={(e) => setRangeTo(e.target.value)}
                    placeholder="To verse"
                    inputMode="numeric"
                    aria-label="Range end verse"
                  />
                  <button type="button" disabled={isBusy("save-range")} onClick={saveRange}>
                    Save range
                  </button>
                  <button type="button" className="op-button--primary" disabled={isBusy("prepare-range")} onClick={prepareRange}>
                    Prepare range
                  </button>
                </div>
              </div>
              {isBusy("open-chapter") ? (
                <p className="live-brain__hint">Loading chapter&hellip;</p>
              ) : (
                <ul className="library-card-list">{chapterVerses.map(renderVerseCard)}</ul>
              )}
            </>
          )}
        </section>
      )}

      {tab === "search" && (
        <section className="library-panel">
          <div className="live-brain__row">
            <input
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && runSearch()}
              placeholder="e.g. Romans 8:28, Romans 8, or a phrase"
              aria-label="Bible search query"
            />
            <button type="button" className="op-button--primary" disabled={!searchQuery.trim() || isBusy("search")} onClick={runSearch}>
              Search
            </button>
          </div>
          {searchResults.length === 0 ? (
            <p className="live-brain__hint">Search the complete Bible dataset by reference or text.</p>
          ) : (
            <ul className="library-card-list">{searchResults.map(renderVerseCard)}</ul>
          )}
        </section>
      )}

      {tab === "saved" && (
        <section className="library-panel">
          {saved.length === 0 ? (
            <p className="library-page__empty">
              Nothing saved yet. Save a verse or range from Browse or Search to build a reusable list here.
            </p>
          ) : (
            <ul className="library-card-list">
              {saved.map((s) => (
                <li key={s.id} className="library-card library-card--bible">
                  <div className="library-card__header">
                    <strong>{s.referenceDisplay}</strong>
                    <span className="library-card__meta">{s.translationId}</span>
                  </div>
                  {s.note && <p className="library-card__text">{s.note}</p>}
                  <div className="library-card__actions">
                    <button type="button" className="op-button--primary" disabled={isBusy(`prepare-${s.referenceDisplay}`)} onClick={() => prepare(s.referenceDisplay)}>
                      Prepare
                    </button>
                    <button type="button" className="op-button--danger" disabled={isBusy(`delete-${s.id}`)} onClick={() => removeSaved(s.id)}>
                      Remove
                    </button>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </section>
      )}
    </div>
  );
}
