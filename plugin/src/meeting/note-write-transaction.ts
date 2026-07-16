type NoteWriteTransactionOptions<T> = {
  file: T;
  previousPath: string;
  targetPath: string;
  content: string;
  getPath: (file: T) => string;
  rename: (file: T, path: string) => Promise<T>;
  write: (file: T, content: string) => Promise<void>;
};

export async function renameThenWriteNote<T>(options: NoteWriteTransactionOptions<T>): Promise<T> {
  let currentFile = options.file;
  try {
    currentFile = await options.rename(currentFile, options.targetPath);
    await options.write(currentFile, options.content);
    return currentFile;
  } catch (error) {
    let rollbackWarning = "";
    if (options.getPath(currentFile) !== options.previousPath) {
      try {
        await options.rename(currentFile, options.previousPath);
      } catch (rollbackError) {
        rollbackWarning = ` The note rename could not be rolled back: ${formatError(rollbackError)}`;
      }
    }
    throw new Error(`Failed to write and rename the meeting note: ${formatError(error)}${rollbackWarning}`);
  }
}

function formatError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
