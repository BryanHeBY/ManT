/**
 * @file Supplies focused manual documents that exercise UI rendering edge
 * cases without coupling end-to-end tests to a host's installed man pages.
 */

export function mandocHtmlWithPreInDefinitionList(): string {
  return `
    <html>
      <body>
        <div class="manual-text">
          <section class="Sh">
            <h1 class="Sh" id="OPTIONS">OPTIONS</h1>
            <dl class="Bl-tag">
              <dt><b>-fcond-mismatch</b></dt>
              <dd>
                Allow conditional expressions with mismatched types.
                <pre>        #define abs(n)          __builtin_abs ((n))
        #define strcpy(d, s)    __builtin_strcpy ((d), (s))</pre>
                More text after the example.
              </dd>
            </dl>
          </section>
        </div>
      </body>
    </html>
  `;
}

/** Minimal real-world shape emitted by bundled mandoc for clang(1). */
export function mandocClangOptionsHtml(): string {
  return `
    <html>
      <body>
        <div class="manual-text">
          <section class="Sh">
            <h1 class="Sh" id="OPTIONS">OPTIONS</h1>
            <section class="Ss">
              <h2 class="Ss" id="Stage_Selection_Options">Stage Selection Options</h2>
              <div class="Bd-indent">
                <dl class="Bl-tag">
                  <dt><b>-E</b></dt>
                  <dd>Run the preprocessor stage.</dd>
                </dl>
              </div>
              <br/>
              <div class="Bd-indent">
                <dl class="Bl-tag">
                  <dt><b>-fsyntax-only</b></dt>
                  <dd>Run the preprocessor, parser and semantic analysis stages.</dd>
                </dl>
              </div>
            </section>
          </section>
        </div>
      </body>
    </html>
  `;
}

/** Nested display shape used for clang's language-standard aliases. */
export function mandocClangStandardsHtml(): string {
  return `
    <html><body><div class="manual-text">
      <section class="Sh">
        <h1 class="Sh" id="OPTIONS">OPTIONS</h1>
        <section class="Ss">
          <h2 class="Ss" id="Language_Selection">Language Selection</h2>
          <div class="Bd-indent">
            <pre><b>c89</b>\n<b>c90</b>\n<b>iso9899:1990</b></pre>
            <p class="Pp"></p>
            <div class="Bd-indent">
              <div class="Bd-indent">ISO C 1990</div><br/>
            </div>
            <br/>
            <pre><b>iso9899:199409</b></pre>
            <p class="Pp"></p>
            <div class="Bd-indent">
              <div class="Bd-indent">ISO C 1990 with amendment 1</div><br/>
            </div>
          </div>
        </section>
      </section>
    </div></body></html>
  `;
}
