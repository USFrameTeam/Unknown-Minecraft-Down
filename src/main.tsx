import React from "react";
import ReactDOM from "react-dom/client";
// 仅引入 latin 子集：界面像素字只用于拉丁字符（数字/英文标题），中文回退系统字体
import "@fontsource/press-start-2p/latin-400.css";
import App from "./App";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
