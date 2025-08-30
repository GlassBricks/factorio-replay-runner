import * as tstl from "typescript-to-lua"
import ts from "typescript"

let exportLines = ["local ____exports = {}", "return ____exports"]

/**
 * @type tstl.Plugin
 */
const plugin = {
  beforeEmit: (program, opitons, emitHost, files) => {
    /**
     * @type ts.Diagnostic[]
     */
    const diagnostics = []
    for (const file of files) {
      if (file.outputPath.endsWith("main.lua")) {
        const lineSep = file.code.includes("\r\n") ? "\r\n" : "\n"
        function removeLine(line) {
          file.code = file.code.replace(line + lineSep, "")
        }
        for (const line of exportLines) {
          removeLine(line)
        }
      } else {
        // check not in exportLines
        for (const line of exportLines) {
          if (file.code.includes(line)) {
            diagnostics.push({
              category: ts.DiagnosticCategory.Error,
              code: 200001,
              messageText: `File ${file.outputPath} contains "${line}"`,
            })
          }
        }
      }
    }
    return diagnostics
  },
}

export default plugin
