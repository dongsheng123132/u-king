// 图片模块声明（vite 的 client 类型里有，但本项目 tsconfig 没引入 vite/client）
declare module "*.jpg" {
  const src: string;
  export default src;
}
