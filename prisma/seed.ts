import { PrismaClient } from "@prisma/client";
import bcrypt from "bcryptjs";

const prisma = new PrismaClient();

async function main() {
  const existingAdmin = await prisma.user.findUnique({
    where: { username: "admin" },
  });

  if (existingAdmin) {
    console.log("管理员账号已存在，跳过初始化");
    return;
  }

  const password = process.env.ADMIN_INITIAL_PASSWORD || "admin123";
  const passwordHash = await bcrypt.hash(password, 10);

  await prisma.user.create({
    data: {
      username: "admin",
      passwordHash,
    },
  });

  console.log(`管理员账号创建成功 - 用户名: admin, 密码: ${password}`);
}

main()
  .catch((e) => {
    console.error("Seed 执行失败:", e);
    process.exit(1);
  })
  .finally(async () => {
    await prisma.$disconnect();
  });
