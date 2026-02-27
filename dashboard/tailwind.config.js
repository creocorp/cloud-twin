/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        aws: "#FF9900",
        azure: "#0078D4",
        gcp: "#4285F4",
      },
    },
  },
  plugins: [],
};
