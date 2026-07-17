import type { AnchorHTMLAttributes, ImgHTMLAttributes, KeyboardEvent, MouseEvent, ReactNode, Ref } from "react";
import ReactMarkdown, { type Components, type UrlTransform } from "react-markdown";
import remarkGfm from "remark-gfm";

interface MarkdownPreviewProps {
  markdown: string;
  onExternalLink(url: string): void;
  elementRef?: Ref<HTMLElement>;
  hidden?: boolean;
}

function strictPreviewUrl(url: string, key: string): string | undefined {
  if (key === "href") {
    if (/^#[\p{L}\p{N}_.:-]+$/u.test(url)) return url;
    try {
      const parsed = new URL(url);
      if (
        (parsed.protocol === "http:" || parsed.protocol === "https:")
        && parsed.host.length > 0
        && parsed.username.length === 0
        && parsed.password.length === 0
        && (url.startsWith("http://") || url.startsWith("https://"))
      ) {
        return url;
      }
    } catch {
      return undefined;
    }
    return undefined;
  }

  if (key === "src") {
    try {
      const parsed = new URL(url);
      if (parsed.protocol === "asset:" && parsed.host === "localhost") return url;
    } catch {
      return undefined;
    }
  }
  return undefined;
}

function PreviewLink({ href, children, onExternalLink, ...props }: AnchorHTMLAttributes<HTMLAnchorElement> & {
  onExternalLink(url: string): void;
}) {
  if (!href) return <span>{children}</span>;
  if (href.startsWith("#")) return <a {...props} href={href}>{children}</a>;
  const follow = (event: MouseEvent<HTMLAnchorElement>) => {
    event.preventDefault();
    onExternalLink(href);
  };
  const followWithKeyboard = (event: KeyboardEvent<HTMLAnchorElement>) => {
    if (event.key !== "Enter") return;
    event.preventDefault();
    onExternalLink(href);
  };
  return <a {...props} data-export-href={href} role="link" tabIndex={0} onClick={follow} onKeyDown={followWithKeyboard}>{children}</a>;
}

function PreviewImage({ src, alt, ...props }: ImgHTMLAttributes<HTMLImageElement>) {
  return src ? <img {...props} src={src} alt={alt ?? ""} /> : <span>{alt}</span>;
}

function components(onExternalLink: (url: string) => void): Components {
  return {
    a: ({ node: _node, ...props }) => <PreviewLink {...props} onExternalLink={onExternalLink} />,
    img: ({ node: _node, ...props }) => <PreviewImage {...props} />,
    li: ({ node: _node, className, children, ...props }) => (
      <li {...props} className={className}>
        {className?.includes("task-list-item") ? <label>{children as ReactNode}</label> : children}
      </li>
    ),
  };
}

export function MarkdownPreview({ markdown, onExternalLink, elementRef, hidden = false }: MarkdownPreviewProps) {
  const transform: UrlTransform = (url, key) => strictPreviewUrl(url, key);
  return (
    <section ref={elementRef} className="preview-pane markdown-body" aria-label="Vista previa" hidden={hidden}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        skipHtml
        urlTransform={transform}
        components={components(onExternalLink)}
      >
        {markdown}
      </ReactMarkdown>
    </section>
  );
}
