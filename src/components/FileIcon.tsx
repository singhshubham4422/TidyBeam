import { AudioLines, File, FileArchive, FileCode2, FileImage, FileText, Film, Folder, FolderSymlink } from "lucide-react";
import { AUDIO_EXTENSIONS, IMAGE_EXTENSIONS, VIDEO_EXTENSIONS, type FileEntry } from "../api";

const CODE = new Set(["rs", "ts", "tsx", "js", "jsx", "json", "py", "go", "html", "css", "c", "cpp", "h", "java", "kt", "swift", "sh", "toml", "yaml", "yml", "xml", "sql"]);
const DOCS = new Set(["txt", "md", "pdf", "doc", "docx", "rtf", "odt", "pages", "csv", "xlsx", "xls", "pptx", "key"]);
const ARCHIVES = new Set(["zip", "7z", "rar", "tar", "gz", "bz2", "xz", "zst", "tgz"]);

export function FileIcon({ entry, size = 18 }: { entry: FileEntry; size?: number }) {
  const props = { size, strokeWidth: 1.75, "aria-hidden": true } as const;
  if (entry.is_dir) {
    return entry.is_symlink ? <FolderSymlink {...props} className="icon-folder" /> : <Folder {...props} className="icon-folder" />;
  }
  const ext = entry.extension;
  const Icon = IMAGE_EXTENSIONS.has(ext) ? FileImage
    : VIDEO_EXTENSIONS.has(ext) || ext === "mkv" || ext === "avi" ? Film
    : AUDIO_EXTENSIONS.has(ext) ? AudioLines
    : ARCHIVES.has(ext) ? FileArchive
    : CODE.has(ext) ? FileCode2
    : DOCS.has(ext) ? FileText
    : File;
  return <Icon {...props} className="icon-file" />;
}
