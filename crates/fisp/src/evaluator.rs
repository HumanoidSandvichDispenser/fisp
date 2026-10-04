use std::{io, sync::mpsc, thread};

use fisp_evaluator::runtime::Runtime;

type Job = Box<dyn FnOnce(&mut Runtime) + Send>;

pub struct Evaluator {
    jobs: mpsc::Sender<Job>,
}

impl Evaluator {
    pub fn spawn() -> io::Result<Self> {
        let (jobs, rx) = mpsc::channel::<Job>();

        thread::Builder::new()
            .name("fisp-evaluator".to_owned())
            .stack_size(256 << 20)
            .spawn(move || {
                let mut runtime = Runtime::new();
                rx.into_iter().for_each(|job| job(&mut runtime));
            });

        Ok(Evaluator { jobs })
    }

    pub fn run<T: Send + 'static>(&self, f: impl FnOnce(&mut Runtime) -> T + Send + 'static) -> T {
        let (tx, rx) = mpsc::sync_channel(1);

        self.jobs
            .send(Box::new(move |runtime| {
                let result = f(runtime);
                tx.send(result).unwrap();
            }))
            .expect("evaluator thread should remain alive");

        rx.recv().expect("evaluator thread should remain alive")
    }
}
