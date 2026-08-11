/** Mock plugin-opener cho preview: chỉ log ra console thay vì mở browser. */
export async function openUrl(url: string): Promise<void> {
  console.log("[preview] openUrl:", url);
}
