import type { NotFoundContext } from '../router';

export function notFoundPage(
  _ctx: NotFoundContext,
  container: HTMLElement
): void {
  container.innerHTML = `
    <div class="empty-state mt-6">
      <h2>Page not found</h2>
      <p>The page you are looking for does not exist.</p>
      <div style="margin-top:16px;">
        <a href="/" class="btn btn-primary">Go home</a>
        <a href="/search" class="btn btn-secondary" style="margin-left:8px;">Search packages</a>
      </div>
    </div>
  `;
}
