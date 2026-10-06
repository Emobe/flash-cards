// Vite's `?raw` import, which gives a file's text. Used to test the trusted card frame page.
declare module "*?raw" {
  const text: string;
  export default text;
}
